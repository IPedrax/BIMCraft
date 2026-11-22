from pathlib import Path
import os,json,subprocess,sys
import argparse
parser=argparse.ArgumentParser(description="Build original bridge examples, flex instance inputs and capture before/after renders.")
parser.add_argument('--cli',required=True,type=Path,help='BIMCraft Rust CLI built from this branch')
parser.add_argument('--out',required=True,type=Path)
args=parser.parse_args()
R=Path(__file__).resolve().parents[2]
S=args.out.resolve();S.mkdir(parents=True,exist_ok=True)
SC=Path(os.environ['BIMCRAFT_SOLIDCRAFT_CLI']);BC=args.cli.resolve()
O=S/'models';O.mkdir(exist_ok=True)
sys.path.insert(0,str(R/'tools/solidcraft'));import publisher
cases={
 'Window':dict(change={'Width':1800,'Height':1200,'LiteCount':5,'TransomHeight':800}),
 'Door':dict(change={'Width':1100,'Height':2400,'LowerPanelHeight':900,'HandleHeight':1050}),
 'Light':dict(change={'FixtureWidth':600,'CageHeight':650,'Drop':1200,'CandleCount':6}),
 'DrawerBase':dict(change={'SDH_Base_drawer_count':4,'SDH_Base_width':914.4}),
 'TwoDoorBase':dict(change={'SDH_Base_shelf_count':3,'SDH_Base_show_fronts':False,'SDH_Base_width':914.4})}
def run(commands,source=None):
 script=O/'run.commands';script.write_text('\n'.join(c+' '+json.dumps(p) for c,p in commands))
 p=subprocess.run([BC,'run',*([source] if source else []),'--script',script],text=True,capture_output=True,timeout=240)
 if p.returncode: raise RuntimeError(p.stderr[-4000:])
 return [json.loads(x) for x in p.stdout.splitlines()]
results={}
for key,c in cases.items():
 family,catalog,_=publisher.discover(R/f'examples/solidcraft/{key}.sc',SC)
 fields=publisher.rows(catalog);m=json.loads((R/f'examples/solidcraft/{key}.mapping.json').read_text());sel=m['parameters']
 publisher.bind(family,fields,{f['name']:sel.get(f['name'],{}).get('scope','source') for f in fields},category=m['category'],groups={n:v['group']for n,v in sel.items()},kinds={n:v.get('kind','auto')for n,v in sel.items()},dimensions=m['dimensions'])
 f=O/f'{key}.bimf';f.write_text(json.dumps(family));name=family['name'];base=O/f'{key}-before.bimcraft';after=O/f'{key}-after.bimcraft'
 steps=[('family',{'path':str(f)})]
 if key in ('Window','Door'):
  steps += [('wall',{'points':[[-2200,0],[2200,0]],'height':3300}), (m['category'],{'at':[0,0],'family':name,**({'sill':700}if key=='Window'else{})})]
 else: steps += [('component',{'family':name,'at':[0,0]})]
 steps += [('graphics',{'view':'{3D}','hide':['walls','levels','grids']}),('save',{'path':str(base)})]
 run(steps)
 doc=json.loads(base.read_text());item=next(e for e in doc['elements'].values() if e.get('category') in ('component','window','door'))
 id=item['guid']
 run([('modify',{'id':id,'params':c['change']}),('save',{'path':str(after)}),('undo',{}),('save',{'path':str(O/f'{key}-undo.bimcraft')}),('redo',{}),('save',{'path':str(O/f'{key}-redo.bimcraft')})],base)
 for state in ['before','after','undo','redo']:
  p=O/f'{key}-{state}.bimcraft'
  run([('save',{'path':str(p.with_suffix('.glb'))})],p)
  if state in ['before','after']:
   run([('render',{'view':'{3D}','width':1000,'height':1000,'azimuth':-70,'elevation':15,'path':str(O/f'{key}-{state}.png')})],p)
 print('DONE',key,flush=True)
 results[key]=dict(source=key+'.sc',family=name,category=m['category'],instance_changes=c['change'],guid=id)
 (S/'showcase-results.json').write_text(json.dumps(results,indent=2))
