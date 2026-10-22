//! Renderer-neutral retained geometry and picking for the experimental GPU viewport.

use std::collections::BTreeMap;

use super::{Camera, EDGE, P3, Scene, add, cross, dot, mul, sub, unit};
use crate::{graphics::{Look, VisualStyle}, model::Id, render::Rgb};

/// One retained vertex. Coordinates are relative to Mesh::origin; modeling remains f64.
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub position: [f32;3],
    pub normal: [f32;3],
    pub color: [f32;3],
    /// Index into Mesh::owners, plus one (zero is background).
    pub owner: u32,
    pub plane: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit { pub id: Id, pub face: &'static str }

/// GPU-neutral data. No graphics API or UI dependency is introduced into the engine.
pub struct Mesh {
    pub origin: P3,
    pub radius: f64,
    pub vertices: Vec<Vertex>,
    pub owners: Vec<(Id,Rgb)>,
    triangles: Vec<PickTriangle>,
    nodes: Vec<Node>,
    order: Vec<usize>,
}

struct PickTriangle { p: [P3;3], hit: Hit, source: usize }
struct Node { lo: P3, hi: P3, range: std::ops::Range<usize>, children: Option<[usize;2]> }

impl Mesh {
    pub fn new(scene: &Scene, look: &Look) -> Result<Self,String> {
        if look.style()!=VisualStyle::Shaded { return Err("Other visual styles use CPU rendering".into()); }
        let shown: Vec<_> = scene.tris.iter().enumerate().filter(|(_,t)|look.shows(t.owner)).collect();
        if shown.iter().any(|(_,t)|look.transparency(t.owner)>0) {
            return Err("Transparent views use CPU rendering".into());
        }
        if shown.iter().any(|(_,t)|scene.skins.get(t.tex as usize).is_some_and(Option::is_some)) {
            return Err("Textured views use CPU rendering".into());
        }
        // Bound the retained vertex allocation independently of GPU device limits.
        if shown.len()>4_000_000 { return Err("This view exceeds the experimental GPU geometry limit".into()); }
        let first=shown.first().map_or([0.0;3],|(_,t)|t.p[0]);
        let (lo,hi)=shown.iter().flat_map(|(_,t)|t.p).fold((first,first),|(lo,hi),p| (
            std::array::from_fn(|i|lo[i].min(p[i])),std::array::from_fn(|i|hi[i].max(p[i]))));
        let origin=mul(add(lo,hi),0.5);
        let radius=dot(sub(hi,lo),sub(hi,lo)).sqrt()*0.5;
        let mut mesh=Self {origin,radius,vertices:Vec::with_capacity(shown.len()*3),owners:Vec::new(),triangles:Vec::new(),nodes:Vec::new(),order:Vec::new()};
        let mut owners=BTreeMap::new();
        for (i,t) in shown {
            let owner=*owners.entry(t.owner).or_insert_with(|| {
                mesh.owners.push((t.owner,look.edge(t.owner,EDGE))); mesh.owners.len() as u32
            });
            let Rgb(r,g,b)=look.surface(t.owner,t.color);
            let color=[r,g,b].map(|v|f32::from(v)/255.0);
            let normal=unit(cross(sub(t.p[1],t.p[0]),sub(t.p[2],t.p[0])));
            let smooth=scene.smooth.get(i).and_then(Option::as_deref);
            for (k,p) in t.p.iter().enumerate() {
                mesh.vertices.push(Vertex {position:sub(*p,origin).map(|v|v as f32),normal:smooth.map_or(normal,|n|n[k]).map(|v|v as f32),
                    color,owner,plane:scene.planes.get(i).copied().unwrap_or(i) as u32+1});
            }
            mesh.triangles.push(PickTriangle {p:t.p,hit:Hit{id:t.owner,face:scene.faces.get(i).copied().unwrap_or("")},source:i});
        }
        mesh.order=(0..mesh.triangles.len()).collect();
        if !mesh.order.is_empty() { mesh.build(0..mesh.order.len()); }
        Ok(mesh)
    }

    /// Camera vectors for a portable orthographic shader. Target subtraction is done in f64.
    pub fn camera(&self, cam: &Camera) -> [[f32;4];4] {
        let (back,right,up)=cam.axes();
        let target=sub(cam.target,self.origin);
        let depth=(self.radius+dot(target,target).sqrt()+1.0).max(1.0);
        let v=|a:P3,w:f64|[a[0] as f32,a[1] as f32,a[2] as f32,w as f32];
        [v(right,2.0*cam.scale/f64::from(cam.width.max(1))),v(up,2.0*cam.scale/f64::from(cam.height.max(1))),
            v(back,0.5/depth),v(target,0.0)]
    }

    fn build(&mut self, range: std::ops::Range<usize>) -> usize {
        let points=range.clone().flat_map(|k|self.triangles[self.order[k]].p);
        let (lo,hi)=points.fold(([f64::INFINITY;3],[f64::NEG_INFINITY;3]),|(lo,hi),p| (
            std::array::from_fn(|i|lo[i].min(p[i])),std::array::from_fn(|i|hi[i].max(p[i]))));
        let id=self.nodes.len(); self.nodes.push(Node{lo,hi,range:range.clone(),children:None});
        if range.len()>8 {
            let axis=(0..3).max_by(|&a,&b|(hi[a]-lo[a]).total_cmp(&(hi[b]-lo[b]))).unwrap_or(0);
            let triangles=&self.triangles;
            let center=|i:usize|triangles[i].p.iter().map(|p|p[axis]).sum::<f64>();
            self.order[range.clone()].select_nth_unstable_by(range.len()/2,|&a,&b|center(a).total_cmp(&center(b)).then(a.cmp(&b)));
            let mid=range.start+range.len()/2;
            let children=[self.build(range.start..mid),self.build(mid..range.end)];
            self.nodes[id].children=Some(children);
        }
        id
    }

    /// Orthographic ray picking through a BVH. No framebuffer download or GPU synchronization.
    pub fn pick(&self, cam: &Camera, x: f64, y: f64) -> Option<Hit> {
        if !x.is_finite() || !y.is_finite() || x<0.0 || y<0.0 || x>=f64::from(cam.width) || y>=f64::from(cam.height) || self.nodes.is_empty() { return None; }
        let (back,_,_)=cam.axes();
        let target=sub(cam.target,self.origin);
        let far=self.radius+dot(target,target).sqrt()+1.0;
        let origin=add(cam.unproject((x.floor()+0.5,y.floor()+0.5)),mul(back,far));
        let direction=mul(back,-1.0);
        let mut stack=vec![0usize];
        let mut found:Option<(f32,usize,Hit)>=None;
        while let Some(id)=stack.pop() {
            let node=self.nodes.get(id)?;
            let (mut near,mut far)=(0.0,f64::INFINITY);
            for k in 0..3 {
                if direction[k].abs()<1e-14 {
                    if origin[k]<node.lo[k]-1e-6 || origin[k]>node.hi[k]+1e-6 { far=-1.0; break; }
                } else {
                    let (a,b)=((node.lo[k]-origin[k])/direction[k],(node.hi[k]-origin[k])/direction[k]);
                    near=f64::max(near,a.min(b)); far=f64::min(far,a.max(b));
                }
            }
            if far+1e-6<near { continue; }
            if let Some(children)=node.children { stack.extend(children); continue; }
            for k in node.range.clone() {
                let t=&self.triangles[self.order[k]];
                let (a,b,c)=(t.p[0],t.p[1],t.p[2]);
                let (e1,e2)=(sub(b,a),sub(c,a)); let p=cross(direction,e2); let det=dot(e1,p);
                if det.abs()<1e-12 { continue; }
                let at=sub(origin,a); let u=dot(at,p)/det; let q=cross(at,e1); let v=dot(direction,q)/det;
                if u < -1e-9 || v < -1e-9 || u+v>1.0+1e-9 { continue; }
                let distance=dot(e2,q)/det;
                if distance<0.0 { continue; }
                let depth=cam.project(add(origin,mul(direction,distance))).2 as f32;
                if found.is_none_or(|(z,i,_)|depth>z || depth==z && t.source<i) { found=Some((depth,t.source,t.hit)); }
            }
        }
        found.map(|(_,_,hit)|hit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine::Session, graphics::Look, view3d::{Scene,AZIMUTH,ELEVATION}};
    use serde_json::json;

    #[test]
    fn retained_mesh_picks_the_same_visible_faces_as_the_cpu_image() {
        let mut s=Session::new(); s.script(crate::sample::SAMPLE).unwrap();
        let scene=Scene::new(&s.model);
        let mesh=Mesh::new(&scene,&Look::default()).unwrap();
        let cam=scene.fit(AZIMUTH,ELEVATION,400,300);
        let image=scene.render(&cam,&[]);
        let mut checked=0;
        for y in (10..290).step_by(7) { for x in (10..390).step_by(7) {
            if let Some(id)=image.pick(x as f64,y as f64) && s.model.level(id).is_none() {
                let hit=mesh.pick(&cam,x as f64,y as f64).unwrap();
                assert_eq!(hit.id,id,"{x},{y}"); assert_eq!(Some(hit.face).filter(|s|!s.is_empty()),image.face(x as f64,y as f64)); checked+=1;
            }
        }}
        assert!(checked>50);
        assert!(mesh.pick(&cam,-1.0,0.0).is_none());
        assert!(mesh.pick(&cam,f64::NAN,0.0).is_none());
        s.execute("visual_style",&json!({"style":"wireframe"})).unwrap();
        assert!(Mesh::new(&scene,&Look::of_3d(&s.model)).is_err());
    }

    #[test]
    fn hidden_geometry_is_not_uploaded_or_picked_and_large_origins_are_rebased() {
        let mut s=Session::new();
        s.execute("column",&json!({"at":[500000000,500000000],"height":3000})).unwrap();
        let scene=Scene::new(&s.model); let mesh=Mesh::new(&scene,&Look::default()).unwrap();
        assert!(mesh.origin[0]>1e8);
        assert!(mesh.vertices.iter().all(|v|v.position.iter().all(|x|x.abs()<10000.0)));
        let cam=scene.fit(AZIMUTH,ELEVATION,400,300);
        assert!(mesh.pick(&cam,200.0,150.0).is_some());
        s.execute("graphics",&json!({"view":"{3D}","hide":["column"]})).unwrap();
        let hidden=Mesh::new(&scene,&Look::of_3d(&s.model)).unwrap();
        assert!(hidden.vertices.is_empty()); assert!(hidden.pick(&cam,200.0,150.0).is_none());
    }
}
