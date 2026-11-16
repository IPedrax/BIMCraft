// Properties a person reads: names instead of GUIDs (a level by its name, a host as the element it is, a link that selects
// it), lengths in millimetres, areas and volumes with their units, and the rows grouped as a model's properties are:
// identity, constraints, dimensions, location, the Revit parameters an import carried, and the family's values.

import { fold } from './panels.js';

const WORDS = { curtainwall: 'Curtain wall', slopedglazing: 'Sloped glazing', structuralcolumn: 'Structural column', stripfooting: 'Strip footing',
  isolatedfooting: 'Isolated footing', structuralframing: 'Structural framing', cabletray: 'Cable tray', ductfitting: 'Duct fitting',
  pipefitting: 'Pipe fitting', plumbingfixture: 'Plumbing fixture', toposolid: 'Toposolid', modelline: 'Model line', modeltext: 'Model text' };
export const catLabel = c => WORDS[c] || (c ? c[0].toUpperCase() + c.slice(1).replace(/_/g, ' ') : 'Other');
const plural = w => /s$/.test(w) ? w : /y$/.test(w) ? w.slice(0, -1) + 'ies' : /(ch|sh|x)$/.test(w) ? w + 'es' : w + 's';
// The category a person knows the element by: Revit's, where the import kept it ("Casework", "Curtain Panels"), else ours.
export const category = n => n.userData.properties?.params?.Category || plural(catLabel(n.userData.category));
export const typeName = n => {
  const p = n.userData.properties || {};
  return p.params?.['Family and Type'] || (p.family && p.type ? `${p.family}: ${p.type}` : p.type || p.family || catLabel(n.userData.category));
};
export const shortName = n => {
  const p = n.userData.properties || {};
  return (p.type || p.family || catLabel(n.userData.category)) + (p.mark ? ` · ${p.mark}` : '');
};

const LENGTH = new Set(['height', 'width', 'length', 'depth', 'thickness', 'offset', 'base_offset', 'end_offset', 'sill', 'diameter', 'riser', 'tread',
  'overhang', 'riser_height', 'mullion_length', 'rise', 'top_offset', 'elevation']);
const GROUPS = [
  ['Identity', ['category', 'family', 'type', 'mark', 'operation', 'system', 'construction', 'structural', 'grade', 'round']],
  ['Constraints', ['level', 'top_level', 'host', 'offset', 'base_offset', 'end_offset', 'sill', 'angle', 'justify', 'attach_top', 'flip_facing', 'flip_hand', 'slope']],
  ['Dimensions', ['length', 'width', 'height', 'depth', 'thickness', 'diameter', 'area_m2', 'volume_m3', 'weight_kg', 'riser', 'riser_height', 'risers', 'tread', 'treads',
    'rise', 'runs', 'landings', 'overhang', 'panel_count', 'mullion_count', 'mullion_length']],
  ['Location', ['at', 'start', 'end', 'axis', 'path', 'boundary', 'outline', 'points', 'ridge', 'top', 'grid']],
];
const NAMES = { area_m2: 'Area', volume_m3: 'Volume', weight_kg: 'Weight', flip_facing: 'Flipped facing', flip_hand: 'Flipped hand', at: 'Insertion point',
  top_level: 'Top level', attach_top: 'Top attached', id: 'Element id', guid: 'GUID', type_guid: 'Type GUID', family_guid: 'Family GUID', round: 'Round' };
const label = k => NAMES[k] || (k[0].toUpperCase() + k.slice(1)).replace(/_/g, ' ');
const space = s => s.replace(/\B(?=(\d{3})+(?!\d))/g, ' ');
export const num = (v, dp = 3) => { const r = +v.toFixed(dp); const [i, f] = String(Math.abs(r)).split('.'); return (r < 0 ? '−' : '') + space(i) + (f ? '.' + f : ''); };
export const length = v => num(v, Math.abs(v) < 10 ? 1 : 0) + ' mm';
const point = p => `E ${num(p[0], 0)}, N ${num(p[1], 0)}` + (p.length > 2 ? `, up ${num(p[2], 0)}` : '') + ' mm';
// What stands folded: the groups a person reads least, then whatever they fold, from one element to the next.
const FOLDED = new Set(['Location', 'Revit parameters', 'Ids']);
const GUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function showProps(app, node, el, title) {
  el.replaceChildren();
  title.textContent = node ? typeName(node) : 'Nothing selected'; title.title = node ? typeName(node) : '';
  if (!node) { el.innerHTML = '<div class="hint">Click an element, or pick one in the model browser.</div>'; return; }
  const p = node.userData.properties || {};
  const levels = new Map(app.levels.flatMap(l => [[String(l.guid), l.name], [String(l.id), l.name]]));
  // A value as words: a GUID as what it names, a number with its unit, a point by its coordinates, a list by its count.
  const value = (k, v) => {
    if (v === null || v === undefined || v === '') return '—';
    if (typeof v === 'boolean') return v ? 'Yes' : 'No';
    if (typeof v === 'string' && GUID.test(v)) {
      if (levels.has(v)) return levels.get(v);
      const n = app.byGuid.get(v);
      if (n) { const a = document.createElement('a'); a.textContent = `${catLabel(n.userData.category)} ${shortName(n)} (${n.userData.id})`; a.onclick = () => app.select(n); return a; }
      return v;
    }
    if (typeof v === 'number') {
      if (k === 'area_m2') return num(v, 2) + ' m²';
      if (k === 'volume_m3') return num(v, 3) + ' m³';
      if (k === 'weight_kg') return num(v, 1) + ' kg';
      if (k === 'angle' || k === 'slope') return num(v, 2) + '°';
      if (LENGTH.has(k)) return length(v);
      return num(v);
    }
    if (Array.isArray(v)) {
      if (v.length && v.every(x => typeof x === 'number')) return point(v);
      if (v.length && v.every(x => Array.isArray(x) && x.every(y => typeof y === 'number'))) return v.length === 2 ? `from ${point(v[0])} to ${point(v[1])}` : `${v.length} points`;
      return `${v.length} ${v.length === 1 ? 'item' : 'items'}`;
    }
    if (typeof v === 'object') return Object.entries(v).map(([a, b]) => `${a}: ${typeof b === 'number' ? num(b) : b}`).join(' · ');
    return String(v);
  };
  const groups = [];
  let rows = null;
  const section = name => groups.push([name, rows = []]);
  const row = (k, v) => {
    const r = document.createElement('div'), a = document.createElement('span'), b = document.createElement('span');
    r.className = 'kv'; a.textContent = k; if (v instanceof Node) b.append(v); else b.textContent = v; r.append(a, b); rows.push(r);
  };
  const used = new Set(['params', 'values', 'connectors', 'id', 'guid', 'type_guid', 'family_guid', 'sweeps', 'profile', 'panels', 'bulges', 'pattern_angle', 'pattern_origin', 'arrow', 'spiral', 'end_tread', 'up']);
  for (const [name, keys] of GROUPS) {
    const here = keys.filter(k => k in p && p[k] !== null && !(typeof p[k] === 'object' && !Array.isArray(p[k])));
    if (!here.length) continue;
    section(name);
    for (const k of here) { used.add(k); row(label(k), k === 'category' ? category(node) : value(k, p[k])); }
  }
  const rest = Object.keys(p).filter(k => !used.has(k) && p[k] !== null && p[k] !== '');
  if (rest.length) { section('Other'); rest.forEach(k => row(label(k), value(k, p[k]))); }
  if (p.connectors?.length) { section('Connectors'); p.connectors.forEach((c, i) => row(`${i + 1}`, `${c.at || ''}${c.flow ? ' · flow ' + c.flow : ''}`)); }
  const params = Object.entries(p.params || {}).filter(([k]) => !['Category', 'Family and Type', 'Reference'].includes(k));
  if (params.length) { section('Revit parameters'); params.sort(([a], [b]) => a.localeCompare(b)).forEach(([k, v]) => row(k, value('', v))); }
  const vals = Object.entries(p.values || {});
  if (vals.length) { section('Family values'); vals.forEach(([k, v]) => row(k, value('', v))); }
  section('Ids'); row('Element id', String(p.id)); row('GUID', p.guid || '—');
  if (p.type_guid) row('Type GUID', p.type_guid);
  if (p.family_guid) row('Family GUID', p.family_guid);
  for (const [name, kids] of groups) el.append(...fold(name, kids, FOLDED.has(name), shut => { if (shut) FOLDED.add(name); else FOLDED.delete(name); }));
}
