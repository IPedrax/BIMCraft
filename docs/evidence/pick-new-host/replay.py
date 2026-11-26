"""Pick New Host through real stdio MCP, optionally attached to the isolated test desktop."""
import json, os, subprocess, sys
from pathlib import Path

binary = Path(sys.argv[1]).resolve()
folder = Path(sys.argv[2]).resolve()
connected = '--connect' in sys.argv
folder.mkdir(parents=True, exist_ok=True)
trace = []
process = None
seq = 0

def start(*args):
    global process
    process = subprocess.Popen([str(binary), 'mcp', *map(str, args)], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    request('initialize', {'protocolVersion': '2025-06-18'})

def request(method, params):
    global seq
    seq += 1
    payload = dict(jsonrpc='2.0', id=seq, method=method, params=params)
    process.stdin.write(json.dumps(payload)+'\n'); process.stdin.flush()
    line = process.stdout.readline()
    assert line, process.stderr.read()
    reply = json.loads(line)
    trace.append(dict(request=payload, response=reply))
    assert 'error' not in reply, reply
    return reply['result']

def execute(command, params=None, error=False):
    r = request('tools/call', dict(name='execute', arguments=dict(command=command, params=params or {})))
    assert bool(r.get('isError')) == error, r
    return r if error else json.loads(r['content'][0]['text'])

def stop():
    process.stdin.close()
    assert process.wait(timeout=30) == 0, process.stderr.read()

def save(stage):
    for ext in ['bimc', 'bimcraft', 'ifc', 'glb']:
        execute('save', {'path': str(folder/f'{stage}.{ext}')})
    for view, suffix in [('Level 1', 'plan'), ('{3D}', '3d')]:
        execute('render', dict(view=view, width=1200, height=800, path=str(folder/f'{stage}-{suffix}.png')))

def rows(category):
    return execute('elements', {'category':category})['elements']

def host_quantities(cut_target):
    for wall, cut, thickness in zip(rows('wall'), [3.69-cut_target,cut_target], [.2,.4]):
        for field, expected in [('area_m2',18-cut),('openings_area_m2',cut),('volume_m3',(18-cut)*thickness)]:
            assert abs(wall[field]-expected)<.001,(wall,field,expected)

try:
    start(*(['--connect'] if connected else []))
    catalog = request('tools/call', {'name':'list_commands', 'arguments':{'filter':'pick_new_host'}})
    assert 'pick_new_host' in catalog['content'][0]['text']
    if not connected:
        for name, thick in [('Source 200',200),('Target 400',400)]:
            execute('type', dict(category='wall', name=name, thickness=thick))
        a = execute('wall', dict(points=[[0,0],[6000,0]], type='Source 200',height=3000,base_offset=400))['ids'][0]
        b = execute('wall', dict(points=[[6000,4000],[0,4000]], type='Target 400',height=3000,base_offset=400))['ids'][0]
        window=execute('window', dict(wall=a,offset=1500,sill=900,mark='WINDOW-KEEP',flip_facing=True))
        # The setting has identical geometry on upright hosts, but remains an instance value to preserve.
        execute('modify', dict(id=window['id'],slanted=True))
        execute('door', dict(wall=a,offset=4000,mark='DOOR-KEEP',flip_hand=True,flip_facing=True))
        execute('tag', {})
        level=rows('level')[0]['guid']
        execute('modify', {'id':level, 'view_range':{'cut':1500}})
    walls=rows('wall')
    assert len(walls)==2 and [x['type'] for x in walls]==['Source 200','Target 400'], walls
    assert [x['mark'] for x in rows('window')+rows('door')]==['WINDOW-KEEP','DOOR-KEEP']
    a,b=[x['id'] for x in walls]
    source,target=[x['guid'] for x in walls]
    before=execute('elements',{})
    opening_rows=rows('window')+rows('door')
    host_quantities(0)
    save('before')
    cut_target=0
    for o in opening_rows:
        execute('pick_new_host',dict(id=o['guid'],wall=target,offset=0),error=True)
        if o == opening_rows[0]: assert execute('elements',{}) == before
        execute('pick_new_host',dict(id=o['guid'],wall=target,offset=o['offset']))
        cut=1.8 if o['category']=='window' else 1.89
        host_quantities(cut_target+cut)
        changed=execute('elements',{})
        execute('undo'); assert next(x for x in rows(o['category']) if x['id']==o['id']) == o
        host_quantities(cut_target)
        execute('pick_new_host',dict(id=o['guid'],wall=target,offset=0),error=True)
        execute('redo'); assert execute('elements',{}) == changed
        cut_target+=cut
        host_quantities(cut_target)
    after=execute('elements',{})
    for o in opening_rows:
        n=next(x for x in rows(o['category']) if x['id']==o['id'])
        for field in ['id','guid','mark','family','type','params','sill','flip_hand','flip_facing','slanted']:
            assert n.get(field)==o.get(field),(field,o,n)
        assert n['host']==target,(n,target)
    save('after')
    (folder/'identities.json').write_text(json.dumps(dict(source=source,target=target,openings=opening_rows),indent=2)+'\n')
    stop()
    if connected:
        start('--connect','--read-only')
        execute('pick_new_host',dict(id=opening_rows[0]['guid'],wall=source,offset=1500),error=True)
        assert execute('elements',{}) == after
        stop()
    # Fresh process reads the saved native file; this applies equally to a desktop-made file.
    start(folder/'after.bimc')
    assert execute('elements',{}) == after
    host_quantities(3.69)
    execute('delete',{'ids':[source]})
    for o in opening_rows:
        assert next(x for x in rows(o['category']) if x['guid']==o['guid'])
    execute('undo'); assert execute('elements',{}) == after
    stop()
    start('--read-only',folder/'after.bimc')
    execute('pick_new_host',dict(id=opening_rows[0]['guid'],wall=source,offset=1500),error=True)
    assert execute('elements',{}) == after
    stop()
    print('PASS: '+('connected desktop' if connected else 'standalone')+' MCP, GUID targets, identity/values, atomic refusal, Undo/Redo, native reopen, old-host deletion, read-only, PNG/IFC/GLB export')
finally:
    (folder/'mcp-trace.json').write_text(json.dumps(trace,indent=2)+'\n')
    if process and process.poll() is None: process.terminate()
