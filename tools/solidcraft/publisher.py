"""Read one saved SolidCraft snapshot with the vanilla evaluator."""
import base64
import io
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile

KINDS = {**dict.fromkeys(('mm','cm','m','in','ft'), 'length'),
         'rad':'angle', 'deg':'angle', '':'number', 'text':'text'}


# BIMCraft Rust component categories and parameter groups in palette order.
CATEGORIES = {
    'door':'Doors', 'window':'Windows',
    'generic':'Generic', 'furniture':'Furniture', 'casework':'Casework',
    'plumbing':'Plumbing Fixtures', 'drain':'Drains', 'pipefitting':'Pipe Fittings',
    'pipeaccessory':'Pipe Accessories', 'equipment':'Electrical Equipment', 'outlet':'Outlets',
    'switch':'Switches', 'airterminal':'Air Terminals', 'earthing':'Earthing Electrodes',
    'inspectionbox':'Inspection Boxes', 'dataoutlet':'Data Outlets', 'voiceoutlet':'Voice Outlets',
    'patchpanel':'Patch Panels', 'rack':'Racks', 'lighting':'Lighting',
}
GROUPS = ('Constraints', 'Construction', 'Graphics', 'Materials and Finishes', 'Dimensions',
          'Identity Data', 'Text', 'Visibility', 'Electrical', 'Mechanical', 'Plumbing',
          'Structural', 'Data', 'Other')
GROUP_LABELS = {label.lower().replace(' ', '-'):label for label in GROUPS}


def discover(source, cli):
    document = source.read_bytes()
    with zipfile.ZipFile(io.BytesIO(document)) as archive:
        name = json.loads(archive.read('document.json'))['name']
    with tempfile.TemporaryDirectory() as directory:
        folder = Path(directory)
        snapshot = folder / 'source.sc'
        snapshot.write_bytes(document)
        script = folder / 'parameters.json'
        script.write_text(json.dumps({'commands':[{'command':'parameters.list','params':{}}]}))
        def read(args):
            reply = subprocess.run([cli,*args], capture_output=True, text=True, check=True, timeout=120)
            return json.loads(reply.stdout)
        catalog = read(['run',script,'--in',snapshot,'--results'])[0]['parameters']
        bodies = read(['eval',snapshot])['bodies']
    if not bodies:
        raise ValueError('SolidCraft has no solid bodies to publish.')
    span = max(b['bbox']['max'][0] for b in bodies) - min(b['bbox']['min'][0] for b in bodies)
    family = {'name':name or source.stem,'category':'generic','types':{'Standard':{}},
              'parameters':[], 'solidcraft':{'document':base64.b64encode(document).decode(), 'inputs':{}}}
    return family, catalog, span + max(500,span/3)


def rows(catalog):
    result = []
    for p in sorted(catalog, key=lambda p:p['source'] != 'user'):
        unit = p['unit']
        linked = p.get('uses') or []
        editable = not (linked or p.get('error')) and unit in KINDS
        value = p.get('text') if unit == 'text' else p['value']
        shown = p.get('text') if unit == 'text' else p.get('display_value')
        display = f'{shown:g} {unit}'.strip() if isinstance(shown,(int,float)) else str(shown or '')
        origin = ' · '.join(str(s) for s in (p['source'],p.get('feature'),p.get('input')) if s is not None)
        note = (p.get('error') or ('Driven by ' + ', '.join(linked) if linked else '')
                or ('' if editable else 'Unsupported unit'))
        result.append(dict(name=p['name'], value=value, unit=unit, expression=str(p['expression']),
                           display=display, origin=origin, comment=p.get('comment') or '', note=note,
                           group='dimensions' if KINDS.get(unit) in ('length', 'angle') else 'other',
                           editable=editable, scope='instance' if editable and p['source']=='user' else 'source'))
    return result


def bind(family, fields, choices, category=None, groups=None, dimensions=None, kinds=None):
    if {f['name'] for f in fields} != choices.keys():
        raise ValueError('The mapping does not match this SolidCraft design.')
    if category is not None:
        if category not in CATEGORIES:
            raise ValueError(f'Unsupported component category: {category}')
        family['category'] = category
    family['parameters'] = []
    family['solidcraft']['inputs'] = {}
    for f in fields:
        name = f['name']
        scope = choices[name]
        if scope == 'source':
            continue
        if not f['editable'] or scope not in ('type','instance'):
            raise ValueError(f'Invalid mapping for {name}.')
        kind = (kinds or {}).get(name, 'auto')
        value = f['value']
        if kind == 'auto':
            kind = KINDS[f['unit']]
        elif f['unit'] != '' or kind not in ('integer', 'yesno'):
            raise ValueError(f'Invalid control kind for {name}.')
        if kind == 'integer' and (not isinstance(value, (int, float)) or not float(value).is_integer()):
            raise ValueError(f'{name} must have a whole-number source default.')
        if kind == 'yesno':
            if value not in (0, 1):
                raise ValueError(f'{name} needs a source default of 0 or 1 for Yes/No.')
            value = bool(value)
        parameter = dict(name=name, kind=kind, value=value, instance=scope=='instance')
        if groups is not None:
            group = groups.get(name, f['group'])
            if group not in GROUP_LABELS:
                raise ValueError(f'Invalid parameter group for {name}.')
            parameter['group'] = group
        family['parameters'].append(parameter)
        family['solidcraft']['inputs'][name] = dict(unit=f['unit'],original=f['value'])
        if kind != KINDS[f['unit']]:
            family['solidcraft']['inputs'][name]['kind'] = kind

    dimensions = dimensions or {}
    parameters = {p['name']:p for p in family['parameters']}
    for role, name in dimensions.items():
        if role not in ('Width', 'Height', 'Depth', 'Overall Depth'):
            raise ValueError(f'Unknown dimension role: {role}')
        if name not in parameters or parameters[name]['kind'] != 'length':
            raise ValueError(f'{role} must reference a published length parameter: {name}')
        if role != name and role in parameters:
            raise ValueError(f'{role} conflicts with an existing source parameter.')
    family['dimension_parameters'] = dict(dimensions)


def main():
    """Explicit mapping; no parameter names or geometry select family behavior."""
    import argparse
    import os
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--cli', default=os.environ.get('BIMCRAFT_SOLIDCRAFT_CLI'))
    parser.add_argument('--mapping', type=Path, help='JSON: category, parameters {name: {scope, group, kind?}}, dimensions')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if not args.cli:
        parser.error('Set BIMCRAFT_SOLIDCRAFT_CLI or pass --cli (vanilla SolidCraft 0.44.1).')
    family, catalog, _ = discover(args.source, args.cli)
    fields = rows(catalog)
    if not args.mapping:
        args.out.write_text(json.dumps(fields, indent=2))
        return
    mapping = json.loads(args.mapping.read_text())
    selected = mapping['parameters']
    unknown = selected.keys() - {f['name'] for f in fields}
    if unknown:
        parser.error(f'Unknown source parameters: {sorted(unknown)}')
    choices = {f['name']: selected.get(f['name'], {}).get('scope', 'source') for f in fields}
    groups = {k:v['group'] for k,v in selected.items()}
    kinds = {k:v.get('kind', 'auto') for k,v in selected.items()}
    bind(family, fields, choices, category=mapping['category'], groups=groups, kinds=kinds, dimensions=mapping.get('dimensions'))
    args.out.write_text(json.dumps(family, indent=2))


if __name__ == '__main__':
    main()
