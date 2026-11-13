// The model browser: the model, then levels, categories, types and elements, each row with an eye that hides or shows it and
// an Only button that shows it alone; a click on an element selects it and flies to it, a click on the model's row shows
// all of it and goes home, and the pointer over a row lights what it holds in the model. The search box finds elements by
// name, type, mark, category, level, id or GUID.
import { category, typeName, shortName } from './props.js';

const EYE = '<svg viewBox="0 0 24 24"><path d="M3 12s3.5-6 9-6 9 6 9 6-3.5 6-9 6-9-6-9-6z"/><circle cx="12" cy="12" r="2.5"/></svg>';

export class Browser {
  constructor(app) {
    this.app = app; this.el = document.getElementById('tree'); this.rows = []; this.byId = new Map();
    const search = document.getElementById('search');
    let timer = null;
    search.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(() => this.search(search.value.trim()), 150); });
    app.on('visibility', () => this.refresh());
    app.on('select', n => this.reveal(n));
  }
  levelName(n) {
    const l = n.userData.properties?.level, lv = this.app.levels.find(x => String(x.guid) === String(l) || String(x.id) === String(l));
    return lv ? lv.name : 'No level';
  }
  build() {
    const groups = new Map();
    for (const n of this.app.elements) {
      const lv = this.levelName(n), cat = category(n), ty = typeName(n);
      if (!groups.has(lv)) groups.set(lv, new Map());
      const g = groups.get(lv); if (!g.has(cat)) g.set(cat, new Map());
      const t = g.get(cat); if (!t.has(ty)) t.set(ty, []); t.get(ty).push(n);
    }
    const order = [...[...this.app.levels].sort((a, b) => a.elevation - b.elevation).map(l => l.name), 'No level'];
    const kids = m => [...m.values()].flatMap(v => Array.isArray(v) ? v : kids(v));
    this.tree = order.filter(l => groups.has(l)).map(l => ({ name: l, map: groups.get(l), nodes: kids(groups.get(l)) }));
    this.el.replaceChildren(); this.rows = []; this.byId = new Map();
    const top = this.group(this.app.title || 'Model', this.app.elements, () => this.tree.map(g => this.group(g.name, g.nodes, () => this.cats(g.map))), false);
    const row = top.firstElementChild; row.classList.add('top'); row.querySelector('.only').remove();
    row.querySelector('.lb').onclick = () => { this.app.select(null); this.app.showAll(); this.app.nav.home(); };
    row.querySelector('.lb').title = 'Show all of the model and go home';
    this.el.append(top); top.toggle(); this.levels = top.lastElementChild;
    document.getElementById('search').value = '';
  }
  cats(map) {
    return [...map].sort(([a], [b]) => a.localeCompare(b)).map(([cat, types]) => {
      const nodes = [...types.values()].flat();
      return this.group(cat, nodes, () => [...types].sort(([a], [b]) => a.localeCompare(b)).map(([ty, ns]) => types.size === 1 && ns.length < 400
        ? this.elementsOf(ns) : this.group(ty, ns, () => this.elementsOf(ns))).flat());
    });
  }
  elementsOf(ns) { return ns.slice(0, 3000).map(n => this.element(n)); }
  // A row for a group: the twisty opens it (its rows are made then), the eye and Only act on all of it.
  group(name, nodes, open, light = true) {
    const wrap = document.createElement('div'), row = document.createElement('div'), kids = document.createElement('div');
    row.className = 'row'; kids.className = 'kids'; kids.hidden = true;
    const tw = document.createElement('button'); tw.className = 'tw'; tw.type = 'button'; tw.textContent = '▶'; tw.title = 'Open';
    const lb = document.createElement('button'); lb.className = 'lb'; lb.type = 'button'; lb.textContent = name; lb.title = name;
    const n = document.createElement('span'); n.className = 'n'; n.textContent = nodes.length;
    const toggle = () => {
      if (!kids.dataset.filled) { kids.append(...open()); kids.dataset.filled = 1; }
      kids.hidden = !kids.hidden; tw.textContent = kids.hidden ? '▶' : '▼';
    };
    tw.onclick = toggle; lb.onclick = toggle;
    row.append(tw, this.eye(nodes), lb, n, this.only(nodes));
    if (light) this.lights(row, nodes);
    wrap.append(row, kids); wrap.toggle = () => { if (kids.hidden) toggle(); };
    return wrap;
  }
  element(node) {
    const row = document.createElement('div'); row.className = 'row'; row.dataset.id = node.userData.id;
    const pad = document.createElement('span'); pad.className = 'tw';
    const lb = document.createElement('button'); lb.className = 'lb'; lb.type = 'button'; lb.textContent = shortName(node); lb.title = typeName(node);
    const n = document.createElement('span'); n.className = 'n'; n.textContent = node.userData.id;
    lb.onclick = () => { this.app.select(node); this.app.nav.focus([node]); };
    row.append(pad, this.eye([node]), lb, n);
    this.lights(row, [node]);
    this.byId.set(node.userData.id, row);
    if (this.app.selected === node) row.classList.add('on');
    return row;
  }
  lights(row, nodes) { row.onpointerenter = () => this.app.hover(nodes); row.onpointerleave = () => this.app.hover(null); }
  eye(nodes) {
    const b = document.createElement('button'); b.className = 'eye'; b.type = 'button'; b.innerHTML = EYE; b.title = 'Hide or show';
    b.onclick = () => this.app.setShown(nodes, !nodes.some(n => n.visible));
    this.rows.push({ b, nodes }); b.classList.toggle('off', !nodes.some(n => n.visible));
    return b;
  }
  only(nodes) {
    const b = document.createElement('button'); b.className = 'only'; b.type = 'button'; b.textContent = 'Only'; b.title = 'Show only these';
    b.onclick = () => { this.app.isolate(nodes); this.app.nav.focus(nodes); };
    return b;
  }
  refresh() { this.rows = this.rows.filter(r => r.b.isConnected); this.rows.forEach(r => r.b.classList.toggle('off', !r.nodes.some(n => n.visible))); }
  // The selected element's row, opened to and lit, when the browser is open.
  reveal(node) {
    this.el.querySelectorAll('.row.on').forEach(r => r.classList.remove('on'));
    if (!node) return;
    if (!this.byId.get(node.userData.id)?.isConnected && !document.getElementById('browser').hidden && !this.searching) {
      const lv = this.levelName(node), cat = category(node), ty = typeName(node);
      const find = (parent, name) => [...parent.children].find(w => w.firstElementChild?.querySelector('.lb')?.textContent === name);
      const a = this.levels && find(this.levels, lv); a?.toggle(); const kidsA = a?.lastElementChild;
      const b = kidsA && find(kidsA, cat); b?.toggle(); const kidsB = b?.lastElementChild;
      const c = kidsB && find(kidsB, ty); c?.toggle();
    }
    const row = this.byId.get(node.userData.id);
    if (row?.isConnected) { row.classList.add('on'); row.scrollIntoView({ block: 'nearest' }); }
  }
  search(q) {
    this.searching = !!q;
    if (!q) { this.build(); return; }
    // Every word must be found; a mark or id given whole ranks first, a GUID is searched only with 8 or more of its characters.
    const words = q.toLowerCase().split(/\s+/);
    const scored = [];
    for (const n of this.app.elements) {
      const p = n.userData.properties || {};
      const hay = [shortName(n), typeName(n), category(n), this.levelName(n), p.params?.Description].join(' ').toLowerCase();
      const whole = [String(p.mark ?? '').toLowerCase(), String(p.id)];
      let score = 0;
      const ok = words.every(w => {
        if (whole.includes(w)) { score += 3; return true; }
        if (hay.includes(w)) { score += hay.split(/[^a-z0-9]+/).some(t => t.startsWith(w)) ? 2 : 1; return true; }
        if (String(p.id).startsWith(w)) { score += 1; return true; }
        return w.length >= 8 && String(p.guid).toLowerCase().includes(w);
      });
      if (ok) scored.push([score, n]);
    }
    const found = scored.sort((a, b) => b[0] - a[0]).map(([, n]) => n);
    this.el.replaceChildren(); this.rows = []; this.byId = new Map();
    const head = document.createElement('div'); head.className = 'hint';
    head.textContent = found.length ? `${found.length} found${found.length > 300 ? ', the first 300 listed' : ''}` : 'Nothing found.';
    this.el.append(head);
    if (found.length) this.el.append(this.group(`All ${found.length} found`, found, () => []).firstElementChild);
    this.el.append(...found.slice(0, 300).map(n => this.element(n)));
  }
}
