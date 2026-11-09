"""Unreal Editor Python commandlet. Run on the isolated W1 receiver project."""
import os
from pathlib import Path
import json
import shutil
import sys
import xml.etree.ElementTree as ET
import unreal as ue

root = Path(os.environ['W1_WORK'])
if not __debug__:
    raise RuntimeError('This test requires Python assertions')
sys.path.insert(0, str(root))
from write_scene import write_scene

fixture = root / 'fixture'
live = root / 'live'
evidence = root / 'evidence'
evidence.mkdir()
folder = '/Game/W1'
level = folder + '/ReimportLab'
levels = ue.get_editor_subsystem(ue.LevelEditorSubsystem)
actors = ue.get_editor_subsystem(ue.EditorActorSubsystem)

def make_siding():
    tools = ue.AssetToolsHelpers.get_asset_tools()
    mat = tools.create_asset('M_DeepTeal_LapSiding', folder, ue.Material, ue.MaterialFactoryNew())
    lib = ue.MaterialEditingLibrary
    def expr(kind, x, y):
        return lib.create_material_expression(mat, kind, x, y)
    def connect(src, output, dest, input_name):
        assert lib.connect_material_expressions(src, output, dest, input_name)
    uv = expr(ue.MaterialExpressionTextureCoordinate, -900, 0)
    g = expr(ue.MaterialExpressionComponentMask, -720, 0)
    g.set_editor_property('r', False)
    g.set_editor_property('g', True)
    connect(uv, '', g, '')
    scale = expr(ue.MaterialExpressionMultiply, -540, 0)
    scale.set_editor_property('const_b', 100/15.24)
    connect(g, '', scale, 'A')
    frac = expr(ue.MaterialExpressionFrac, -360, 0)
    connect(scale, '', frac, '')
    ridge = expr(ue.MaterialExpressionPower, -180, 0)
    ridge.set_editor_property('const_exponent', 7)
    connect(frac, '', ridge, '')
    mix = expr(ue.MaterialExpressionLinearInterpolate, 80, 0)
    for name, rgb, y in [('A',(0.024,0.28,0.26),-270), ('B',(0.002,0.02,0.035),-130)]:
        pigment = expr(ue.MaterialExpressionConstant3Vector, -150, y)
        pigment.set_editor_property('constant', ue.LinearColor(*rgb,1))
        connect(pigment, '', mix, name)
    connect(ridge, '', mix, 'Alpha')
    assert lib.connect_material_property(mix, '', ue.MaterialProperty.MP_BASE_COLOR)
    lib.recompile_material(mat)
    assert ue.EditorAssetLibrary.save_loaded_asset(mat)
    return mat

def wall_gap(vertices, indices):
    # Intersect the imported front wall surface with a horizontal line at 130 cm.
    front = min(p[1] for p in vertices)
    spans = []
    for i in range(0,len(indices),3):
        tri = [vertices[j] for j in indices[i:i+3]]
        if any(abs(p[1]-front) > 0.001 for p in tri):
            continue
        hits = []
        for a,b in zip(tri, tri[1:]+tri[:1]):
            if min(a[2],b[2]) < 130 < max(a[2],b[2]):
                hits.append(a[0] + (130-a[2])*(b[0]-a[0])/(b[2]-a[2]))
        if len(hits) == 2:
            spans.append(sorted(hits))
    merged = []
    for lo,hi in sorted(spans):
        if merged and lo < merged[-1][1]+0.001:
            merged[-1][1] = max(merged[-1][1], hi)
        else:
            merged.append([lo,hi])
    assert len(merged) == 2, merged
    assert abs(merged[0][0]) < .001 and abs(merged[1][1]-365.76) < .001
    return [merged[0][1],merged[1][0]]

def capture(phase):
    mesh_actors = [a for a in actors.get_all_level_actors() if isinstance(a,ue.StaticMeshActor)]
    roots = [a for a in actors.get_all_level_actors() if isinstance(a,ue.DatasmithSceneActor)]
    assert len(mesh_actors) == 2 and len(roots) == 1, [a.get_actor_label() for a in mesh_actors]
    source_keys = {node.get('label'): node.get('name') for node in
                   ET.parse(live/'CON03_W1.udatasmith').findall('ActorMesh')}
    rows = {}
    for a in mesh_actors:
        c = a.static_mesh_component
        mesh = c.static_mesh
        label = a.get_actor_label()
        assert a.get_attach_parent_actor() == roots[0]
        verts, idx, normals, uvs, _ = ue.ProceduralMeshLibrary.get_section_from_static_mesh(mesh,0,0)
        tx = c.get_world_transform()
        world = [tx.transform_location(v).to_tuple() for v in verts]
        vertices = [list(p) for p in world]
        assert vertices and len(idx) in (36,144)
        key = ue.DatasmithContentLibrary.get_datasmith_user_data_value_for_key(a,'Datasmith_UniqueId')
        assert key == source_keys[label], (key,source_keys[label])
        row = dict(datasmith_id=key, actor=a.get_path_name(), guid=a.get_editor_property('actor_guid').to_string(),
                   mesh=mesh.get_path_name(), material=c.get_material(0).get_path_name(),
                   slot_count=c.get_num_materials(), triangles=len(idx)//3, uv_count=len(uvs),
                   x_cm=[min(p[0] for p in world),max(p[0] for p in world)],
                   z_cm=[min(p[2] for p in world),max(p[2] for p in world)])
        if label == 'Exterior_Wall':
            row['opening_x_cm'] = wall_gap(vertices, list(idx))
        rows[label] = row
    assert set(rows) == {'Exterior_Wall','W1'}
    doc = dict(phase=phase, engine=ue.SystemLibrary.get_engine_version(), level=level,
               datasmith_root=roots[0].get_path_name(), source=str(live/'CON03_W1.udatasmith'),
               model_actor_count=len(mesh_actors), actors=rows)
    (evidence/(phase+'.json')).write_text(json.dumps(doc,indent=2)+'\n')
    return rows

def export(phase):
    scene = write_scene(fixture/(phase+'.bimcraft'), fixture/(phase+'.glb'), live, root/'w1.dylib', os.environ['W1_VERSION'])
    shutil.copytree(live,evidence/phase)
    return scene

assert levels.new_level(level)
# A front light and camera make the isolated level inspectable in the editor.
actors.spawn_actor_from_class(ue.DirectionalLight, ue.Vector(0,-350,500), ue.Rotator(0,-35,80))
camera = actors.spawn_actor_from_class(ue.CameraActor, ue.Vector(183,-440,135), ue.Rotator(0,-2,90))
camera.set_actor_label('Front elevation')
scene = export('original')
source = ue.DatasmithSceneElement.construct_datasmith_scene_from_file(str(scene))
result = source.import_scene(folder+'/Import')
assert result.import_succeed
scene_path = next(a for a in actors.get_all_level_actors() if isinstance(a,ue.DatasmithSceneActor)).get_editor_property('scene').get_path_name()
wall, = [a for a in actors.get_all_level_actors() if a.get_actor_label()=='Exterior_Wall']
original_material = wall.static_mesh_component.get_material(0).get_path_name()
material = make_siding()
assert original_material != material.get_path_name()
wall.modify()
wall.static_mesh_component.modify()
wall.static_mesh_component.set_material(0,material)
assert levels.save_current_level()
ue.EditorAssetLibrary.save_directory(folder)
before = capture('before')
original_objects = {a.get_actor_label():a for a in actors.get_all_level_actors() if isinstance(a,ue.StaticMeshActor)}
assert before['Exterior_Wall']['material'] == material.get_path_name()

assert export('moved') == scene
existing = ue.DatasmithSceneElement.get_existing_datasmith_scene(scene_path)
assert existing and existing.reimport_scene().import_succeed
ue.EditorAssetLibrary.save_directory(folder)
assert levels.save_current_level()
after = capture('after')
for a in actors.get_all_level_actors():
    if isinstance(a,ue.StaticMeshActor):
        assert a == original_objects[a.get_actor_label()]
        if a == wall:
            assert a.static_mesh_component.get_material(0) == material
assert levels.load_level(level)
reopened = capture('reopened')
for name in before:
    for field in ('actor','datasmith_id','mesh','material','triangles','slot_count'):
        assert before[name][field] == after[name][field] == reopened[name][field], (name,field)
    assert after[name]['guid'] == reopened[name]['guid']
for state, expected in [(before,182.88),(after,223.52),(reopened,223.52)]:
    opening = state['Exterior_Wall']['opening_x_cm']
    window = state['W1']['x_cm']
    for i, side in enumerate((expected-45.72,expected+45.72)):
        assert abs(opening[i]-side)<0.001 and abs(window[i]-side)<0.001
    assert state['Exterior_Wall']['triangles'] == 48 and state['W1']['triangles'] == 12
    assert all(row['slot_count'] == 1 for row in state.values())
    assert all(abs(v-e)<0.001 for v,e in zip(state['W1']['z_cm'],[91.44,182.88]))
(evidence/'PASS.json').write_text(json.dumps(dict(scene=scene_path,level=level,original_material=original_material,material=material.get_path_name(),
             engine=ue.SystemLibrary.get_engine_version(),evidence=str(evidence)),indent=2)+'\n')
ue.log("W1 Unreal reimport PASS " + str(evidence))
