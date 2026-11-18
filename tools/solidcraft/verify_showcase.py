"""Compare reopened BC GLB geometry with vanilla SC's independent model.mesh replies.

Original five showcase fixtures only. The application adapter has no fixture names.
Uses only Python's standard library. A 0.001 mm comparison tolerance accommodates GLB float32.
"""
import argparse
from collections import Counter, defaultdict
from itertools import product
import json
import math
import os
from pathlib import Path
import struct
import subprocess


def glb(path):
    raw = path.read_bytes()
    size = struct.unpack_from('<I', raw, 12)[0]
    doc = json.loads(raw[20:20+size]); buf = raw[28+size:]
    def accessor(index, fmt):
        a = doc['accessors'][index]; v = doc['bufferViews'][a['bufferView']]
        assert a['componentType'] == (5125 if fmt == 'I' else 5126)
        start = v.get('byteOffset', 0) + a.get('byteOffset', 0)
        stride = v.get('byteStride', struct.calcsize('<'+fmt))
        return [struct.unpack_from('<'+fmt, buf, start+i*stride) for i in range(a['count'])]
    triangles = []
    for node in doc['nodes']:
        if node['extras']['category'] not in ('component', 'door', 'window'): continue
        assert not set(node) & {'matrix', 'rotation', 'translation', 'scale'}
        for p in doc['meshes'][node['mesh']]['primitives']:
            points = [(1000*x, -1000*z, 1000*y) for x,y,z in accessor(p['attributes']['POSITION'], '3f')]
            indices = [i for i, in accessor(p['indices'], 'I')]
            triangles += [tuple(points[i] for i in indices[n:n+3]) for n in range(0, len(indices), 3)]
    return triangles


def match(actual, reference, offset):
    bins = defaultdict(list); vertices = []; indices = {}
    def cell(p): return tuple(math.floor(v/.002) for v in p)
    def near(p):
        return (i for delta in product((-1,0,1), repeat=3) for i in bins[tuple(a+b for a,b in zip(cell(p),delta))])
    for p in sorted({tuple(p) for t in reference for p in t}):
        found = next((i for i in near(p) if sum((p[k]-vertices[i][k])**2 for k in range(3)) < 1e-12), None)
        if found is None:
            found = len(vertices); vertices.append(p); bins[cell(p)].append(found)
        indices[p] = found
    mapped = {}; max_error = 0
    for p in {tuple(p) for t in actual for p in t}:
        local = tuple(p[i]-offset[i] for i in range(3)); candidates = list(near(local))
        assert candidates, local
        d2,i = min((sum((local[k]-vertices[i][k])**2 for k in range(3)), i) for i in candidates)
        assert d2 < 1e-6, (local, vertices[i]); mapped[p] = i; max_error=max(max_error, math.sqrt(d2))
    assert Counter(tuple(sorted(mapped[tuple(p)] for p in t)) for t in actual) == Counter(tuple(sorted(indices[tuple(p)] for p in t)) for t in reference)
    return max_error


def main():
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument('output', type=Path); args=parser.parse_args()
    root = Path(__file__).resolve().parents[2]; out=args.output.resolve(); models=out/'models'
    cases=json.loads((out/'showcase-results.json').read_text()); report={}
    cli=os.environ['BIMCRAFT_SOLIDCRAFT_CLI']
    def sc(source, commands, save=None):
        script=out/'oracle.json'; script.write_text(json.dumps({'commands': commands}))
        p=subprocess.run([cli,'run',str(script),'--in',str(source),'--results',*(['--save',str(save)] if save else [])],capture_output=True,text=True,timeout=120)
        assert p.returncode == 0, p.stderr
        return json.loads(p.stdout)
    for name, case in cases.items():
        report[name]={}
        for state in ('before','after'):
            changes = case['instance_changes'] if state=='after' else {}
            mapping=json.loads((root/f'examples/solidcraft/{name}.mapping.json').read_text())['parameters']
            family=json.loads((models/f'{name}.bimf').read_text())
            inputs=family['solidcraft']['inputs']
            expressions={k: str(int(v)) if isinstance(v,bool) else str(v)+(' mm' if inputs[k]['unit'] else '') for k,v in changes.items()}
            evaluated=out/'oracle.sc'
            commands=([{'command':'parameters.change','params':{'changes':expressions}}]if expressions else [])
            commands += [{'command':'component.list','params':{}},{'command':'inspect.measure','params':{}}]
            results=sc(root/f'examples/solidcraft/{name}.sc',commands,evaluated)
            tree,measure=results[-2:]
            names=[n for c in tree['components'] for n in c['bodies']]
            meshes=sc(evaluated,[{'command':'model.mesh','params':{'body':n,'tolerance':.001}} for n in names])
            definitions={m['body']:m for m in meshes}; nodes={c['id']:c for c in tree['components']}
            identity=[[float(i==j) for j in range(4)]for i in range(4)]
            pending=[(0,identity)]; reference=[]; placed=0
            while pending:
                cid,matrix=pending.pop(); node=nodes[cid]
                for body in node['bodies']:
                    mesh=definitions[body]; placed+=1
                    pts=[tuple(sum(matrix[i][j]*p[j] for j in range(3))+matrix[i][3] for i in range(3)) for p in mesh['positions']]
                    reference.extend(tuple(pts[i] for i in t) for t in mesh['triangles'])
                for child in node['children']:
                    local=list(zip(*child['transform']))
                    world=[[sum(matrix[i][k]*local[k][j] for k in range(4)) for j in range(4)]for i in range(4)]
                    pending.append((child['component'],world))
            assert placed==len(measure['bodies'])
            actual=glb(models/f'{name}-{state}.glb')
            offset=(0,0,700 if name=='Window' else 0)
            error=match(actual,reference,offset)
            assert len(actual) <= 400000
            history='undo' if state=='before' else 'redo'
            assert actual == glb(models/f'{name}-{history}.glb'), history
            report[name][state]={'triangles':len(actual),'bodies':len(measure['bodies']),'max_vertex_error_mm':error,'all_triangles_match':True,'history_reopen_exact':True}
        print('PASS geometry/history',name,flush=True)
        (out/'geometry-verification.json').write_text(json.dumps(report,indent=2)+'\n')


if __name__ == '__main__': main()
