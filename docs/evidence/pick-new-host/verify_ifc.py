"""Independent IFC relationship and net-volume oracle for the synthetic MCP fixture."""
import json, math, sys, uuid
from pathlib import Path
import ifcopenshell, ifcopenshell.geom, ifcopenshell.validate
import ifcopenshell.util.shape
folder=Path(sys.argv[1])
ident=json.loads((folder/'identities.json').read_text())
def guid(s): return ifcopenshell.guid.compress(uuid.UUID(s).hex)
result={}
for stage in ['before','after']:
    f=ifcopenshell.open(str(folder/f'{stage}.ifc'))
    log=ifcopenshell.validate.json_logger()
    ifcopenshell.validate.validate(f,log,express_rules=True)
    assert not log.statements, log.statements
    settings=ifcopenshell.geom.settings()
    settings.set(settings.USE_WORLD_COORDS,True)
    volumes={}
    for role, thick in [('source',.2),('target',.4)]:
        wall=f.by_guid(guid(ident[role]))
        shape=ifcopenshell.geom.create_shape(settings,wall)
        volume=ifcopenshell.util.shape.get_volume(shape.geometry)
        # Both ordinary wall cuts: 1.2*1.5 window plus .9*2.1 door.
        openings=(1.2*1.5+.9*2.1) if (role=='source')==(stage=='before') else 0
        expected=(6*3-openings)*thick
        assert math.isclose(volume,expected,abs_tol=1e-6),(stage,role,volume,expected)
        volumes[role]=dict(volume_m3=volume,expected_m3=expected)
    for opening in ident['openings']:
        product=f.by_guid(guid(opening['guid']))
        void=product.FillsVoids[0].RelatingOpeningElement
        host=void.VoidsElements[0].RelatingBuildingElement
        assert host.GlobalId==guid(ident['source' if stage=='before' else 'target'])
        ifcopenshell.geom.create_shape(settings,product)
    result[stage]=dict(express_findings=0,volumes=volumes,opening_host_relations=True)
(folder/'ifc-measurements.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
