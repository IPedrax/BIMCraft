//! Vanilla SC 0.44.1's bounded, read-only mesh API, in exact component placements.
//! The export mesher over-refines long curved faces. Request chord accuracy, never
//! relax it to meet the triangle budget. SC floors chord tolerance at body size / 1e6.
use super::{MAX_BUILT, Piece, evaluator_path};
use crate::family::{P3, mesh};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

const CHORD_MM: f64 = 0.001;
const MAX_NODES: usize = 4096;
const MAX_JSON: u64 = 32 * 1024 * 1024;
type Matrix = [[f64; 4]; 4];
const IDENTITY: Matrix = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];

#[derive(Debug, Deserialize)]
struct Tree {
    components: Vec<Component>,
    pending_moves: Vec<Value>,
}
#[derive(Debug, Deserialize)]
struct Component {
    id: u64,
    bodies: Vec<String>,
    children: Vec<Placement>,
}
#[derive(Debug, Deserialize)]
struct Placement {
    component: u64,
    transform: Matrix,
}
#[derive(Debug, Deserialize)]
struct BodyMesh {
    body: String,
    component: u64,
    frame: String,
    positions: Vec<P3>,
    triangles: Vec<[usize; 3]>,
}

fn read_json(path: &Path) -> Result<Value, String> {
    let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_JSON {
        return Err("SolidCraft mesh reply exceeds the transfer budget".into());
    }
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| format!("Invalid SolidCraft mesh reply: {e}"))
}

fn run(dir: &Path, source: &Path, commands: Vec<Value>, save: Option<&Path>) -> Result<Vec<Value>, String> {
    let script = dir.join("commands.json");
    let reply = dir.join("reply.json");
    let stderr = dir.join("stderr.txt");
    fs::write(&script, json!({"commands": commands}).to_string()).map_err(|e| e.to_string())?;
    let cli = evaluator_path(std::env::var_os("BIMCRAFT_SOLIDCRAFT_CLI"))?;
    let mut command = Command::new(&cli);
    command
        .args(["run"])
        .arg(&script)
        .arg("--in")
        .arg(source)
        .args(["--results", "--json-out"])
        .arg(&reply)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(fs::File::create(&stderr).map_err(|e| e.to_string())?);
    if let Some(save) = save {
        command.arg("--save").arg(save);
    }
    let mut child = command.spawn().map_err(|e| format!("Cannot run the vanilla SolidCraft evaluator at {}: {e}", cli.display()))?;
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Cannot wait for SolidCraft: {e}"));
            }
        }
        if start.elapsed() > std::time::Duration::from_secs(120)
            || fs::metadata(&stderr).is_ok_and(|m| m.len() > 65536)
            || fs::metadata(&reply).is_ok_and(|m| m.len() > MAX_JSON)
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err("SolidCraft exceeded its execution or transfer budget; mesh refused".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    if fs::metadata(&stderr).map_err(|e| e.to_string())?.len() > 65536 {
        return Err("SolidCraft reported excessive diagnostics; mesh refused".into());
    }
    let diagnostic = fs::read_to_string(&stderr).map_err(|e| e.to_string())?;
    // The display planner may report omitted/lenient faces through warnings instead
    // of failing the command. No warned result is silently accepted as a full solid.
    if !status.success() || !diagnostic.trim().is_empty() {
        return Err(format!(
            "SolidCraft could not produce a verified mesh at the requested accuracy: {}",
            diagnostic.chars().take(2048).collect::<String>().trim()
        ));
    }
    read_json(&reply)?.as_array().cloned().ok_or_else(|| "Expected SolidCraft command results".into())
}

pub(super) fn evaluate(bytes: &[u8], changes: &BTreeMap<String, String>) -> Result<Vec<Piece>, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let source = dir.path().join("source.sc");
    let evaluated = dir.path().join("evaluated.sc");
    fs::write(&source, bytes).map_err(|e| e.to_string())?;
    let mut commands = Vec::new();
    if !changes.is_empty() {
        commands.push(json!({"command":"parameters.change", "params":{"changes":changes}}));
    }
    commands.push(json!({"command":"component.list", "params":{}}));
    commands.push(json!({"command":"inspect.measure", "params":{"names_only":true}}));
    let discovery = run(dir.path(), &source, commands, Some(&evaluated))?;
    let count = discovery
        .last()
        .and_then(|r| r.get("bodies"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or("Missing SolidCraft placed-body inventory")?;
    let tree_value = discovery.get(discovery.len().saturating_sub(2)).ok_or("Missing SolidCraft components")?;
    let tree: Tree = serde_json::from_value(tree_value.clone()).map_err(|e| e.to_string())?;
    let definitions = tree.definitions()?;
    if count == 0 || count > MAX_NODES {
        return Err("SolidCraft placed-body count is outside the supported budget".into());
    }
    let mut meshes = BTreeMap::new();
    // A single bounded response never contains the whole family's large mesh.
    let names: Vec<_> = definitions.keys().cloned().collect();
    let mut total = 0usize;
    for batch in names.chunks(8) {
        let commands = batch.iter().map(|name| json!({"command":"model.mesh", "params":{"body":name,"tolerance":CHORD_MM}})).collect();
        let results = run(dir.path(), &evaluated, commands, None)?;
        if results.len() != batch.len() {
            return Err("SolidCraft omitted a requested body mesh".into());
        }
        for (name, result) in batch.iter().zip(results) {
            let body: BodyMesh = serde_json::from_value(result).map_err(|e| e.to_string())?;
            if &body.body != name || definitions.get(name) != Some(&body.component) || body.frame != "component-local-mm" {
                return Err("SolidCraft body mesh does not match its component definition".into());
            }
            let tris = body.checked_triangles()?;
            total = total.checked_add(tris.len()).ok_or("SolidCraft triangle count overflow")?;
            if total > MAX_BUILT {
                return Err(budget_error());
            }
            meshes.insert(name.clone(), tris);
        }
    }
    tree.place(&meshes, count)
}

fn budget_error() -> String {
    format!("SolidCraft result exceeds the {MAX_BUILT}-triangle family limit at the requested accuracy; no coarser mesh was substituted")
}

impl Tree {
    fn definitions(&self) -> Result<BTreeMap<String, u64>, String> {
        if !self.pending_moves.is_empty() {
            return Err("Capture SolidCraft's pending moves before publishing".into());
        }
        if self.components.is_empty() || self.components.len() > MAX_NODES {
            return Err("SolidCraft component count exceeds the budget".into());
        }
        let mut ids = BTreeSet::new();
        let mut bodies = BTreeMap::new();
        for node in &self.components {
            if !ids.insert(node.id) || node.children.len() > MAX_NODES {
                return Err("Invalid SolidCraft component inventory".into());
            }
            for name in &node.bodies {
                if bodies.insert(name.clone(), node.id).is_some() || bodies.len() > MAX_NODES {
                    return Err("Duplicate or excessive SolidCraft body definitions".into());
                }
            }
        }
        if !ids.contains(&0) {
            return Err("Missing SolidCraft root component".into());
        }
        Ok(bodies)
    }

    fn place(&self, bodies: &BTreeMap<String, Vec<[P3; 3]>>, expected: usize) -> Result<Vec<Piece>, String> {
        self.definitions()?;
        let nodes: BTreeMap<_, _> = self.components.iter().map(|n| (n.id, n)).collect();
        let mut todo = vec![(0, IDENTITY, Vec::<u64>::new())];
        let mut pieces = Vec::new();
        let mut visits = 0;
        let mut triangles = 0usize;
        while let Some((id, matrix, mut path)) = todo.pop() {
            visits += 1;
            if visits > MAX_NODES || path.len() >= 64 || path.contains(&id) {
                return Err("Cyclic or excessive SolidCraft placements".into());
            }
            path.push(id);
            let node = nodes.get(&id).ok_or("Missing SolidCraft child component")?;
            for name in &node.bodies {
                let local = bodies.get(name).ok_or("Missing SolidCraft body mesh")?;
                triangles = triangles.checked_add(local.len()).ok_or("SolidCraft triangle count overflow")?;
                if triangles > MAX_BUILT {
                    return Err(budget_error());
                }
                let mirrored = determinant(&matrix) < 0.;
                let tris: Vec<_> = local
                    .iter()
                    .map(|t| {
                        let mut t = t.map(|p| std::array::from_fn(|r| matrix[r][3] + (0..3).map(|c| matrix[r][c] * p[c]).sum::<f64>()));
                        if mirrored {
                            t.swap(1, 2);
                        }
                        t
                    })
                    .collect();
                pieces.push(mesh(&tris, String::new())?);
            }
            for child in node.children.iter().rev() {
                // component.list serializes column-major matrices; occurrence.move accepts rows.
                let transform: Matrix = std::array::from_fn(|r| std::array::from_fn(|c| child.transform[c][r]));
                check_matrix(&transform)?;
                let composed = std::array::from_fn(|r| std::array::from_fn(|c| (0..4).map(|k| matrix[r][k] * transform[k][c]).sum()));
                todo.push((child.component, composed, path.clone()));
            }
        }
        if pieces.len() != expected || pieces.is_empty() {
            return Err("SolidCraft placements do not match the world-body inventory".into());
        }
        Ok(pieces)
    }
}

fn determinant(m: &Matrix) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}
fn check_matrix(m: &Matrix) -> Result<(), String> {
    let rigid = m.iter().flatten().all(|v| v.is_finite())
        && m[3].iter().zip([0., 0., 0., 1.]).all(|(a, b)| (a - b).abs() < 1e-8)
        && (0..3).all(|a| (0..3).all(|b| ((0..3).map(|k| m[k][a] * m[k][b]).sum::<f64>() - if a == b { 1. } else { 0. }).abs() < 1e-7));
    if rigid { Ok(()) } else { Err("SolidCraft placement must be a finite rigid transform".into()) }
}

impl BodyMesh {
    fn checked_triangles(&self) -> Result<Vec<[P3; 3]>, String> {
        if self.positions.is_empty() || self.positions.len() > 8192 || self.triangles.is_empty() || self.triangles.len() > 16384 {
            return Err("SolidCraft body exceeds its mesh transfer budget at the requested accuracy".into());
        }
        if self.positions.iter().flatten().any(|v| !v.is_finite() || v.abs() > crate::model::MAX_COORD) {
            return Err("Invalid SolidCraft mesh coordinate".into());
        }
        let mut welded = BTreeMap::new();
        let ids: Vec<_> = self
            .positions
            .iter()
            .map(|p| {
                // Validation only: returned coordinates are never rounded or moved.
                let key = p.map(|v| (v * 1e6).round() as i64);
                let next = welded.len();
                *welded.entry(key).or_insert(next)
            })
            .collect();
        let mut edges = BTreeMap::<(usize, usize), (usize, i32)>::new();
        let mut tris = Vec::with_capacity(self.triangles.len());
        for indices in &self.triangles {
            let mut t = [[0.; 3]; 3];
            let mut corners = [0; 3];
            for k in 0..3 {
                t[k] = *self.positions.get(indices[k]).ok_or("Invalid SolidCraft mesh index")?;
                corners[k] = *ids.get(indices[k]).ok_or("Invalid SolidCraft mesh index")?;
            }
            for k in 0..3 {
                let (a, b) = (corners[k], corners[(k + 1) % 3]);
                if a == b {
                    return Err("SolidCraft mesh contains a collapsed edge".into());
                }
                let entry = edges.entry((a.min(b), a.max(b))).or_default();
                entry.0 += 1;
                entry.1 += if a < b { 1 } else { -1 };
            }
            tris.push(t);
        }
        if edges.values().any(|&(n, turn)| n != 2 || turn != 0) {
            return Err("SolidCraft mesh is not a closed, consistently oriented solid; family unchanged".into());
        }
        Ok(tris)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tetra() -> BodyMesh {
        BodyMesh {
            body: "arbitrary".into(),
            component: 1,
            frame: "component-local-mm".into(),
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        }
    }
    fn tree() -> Tree {
        serde_json::from_value(json!({"pending_moves":[],"components":[
            {"id":0,"bodies":[],"children":[{"component":1,"transform":IDENTITY}]},
            {"id":1,"bodies":["arbitrary"],"children":[]}
        ]}))
        .unwrap()
    }
    #[test]
    fn body_transfer_is_closed_or_refused_without_repairing_or_rounding_it() {
        let m = tetra();
        let original = m.checked_triangles().unwrap();
        assert_eq!(original[0][1], m.positions[2]);
        let mut m = tetra();
        m.triangles.pop();
        assert!(m.checked_triangles().unwrap_err().contains("closed"));
        let mut m = tetra();
        m.triangles[0].swap(0, 1);
        assert!(m.checked_triangles().is_err());
        let mut m = tetra();
        m.triangles[0][0] = 99;
        assert!(m.checked_triangles().unwrap_err().contains("index"));
        let mut m = tetra();
        m.positions[0][0] = f64::NAN;
        assert!(m.checked_triangles().is_err());
        let mut m = tetra();
        m.triangles = vec![[0, 1, 2]; 16385];
        assert!(m.checked_triangles().unwrap_err().contains("budget"));
    }
    #[test]
    fn nested_repeated_and_mirrored_placements_keep_every_piece_in_world_coordinates() {
        let mut t = tree();
        t.components[0].children[0].transform[3][0] = 10.;
        t.components.push(Component { id: 2, bodies: vec![], children: vec![Placement { component: 1, transform: IDENTITY }] });
        let mut reflected = IDENTITY;
        reflected[0][0] = -1.;
        reflected[3][1] = 20.;
        t.components[0].children.push(Placement { component: 2, transform: reflected });
        let tris = tetra().checked_triangles().unwrap();
        let bodies = BTreeMap::from([("arbitrary".into(), tris)]);
        let p = t.place(&bodies, 2).unwrap();
        assert_eq!(p[0].tris[0][0], [10., 0., 0.]);
        assert_eq!(p[1].tris[0], [[0., 20., 0.], [-1., 20., 0.], [0., 21., 0.]]);
        assert!(t.place(&bodies, 1).unwrap_err().contains("inventory"));
        t.components[1].children.push(Placement { component: 0, transform: IDENTITY });
        assert!(t.place(&bodies, 2).unwrap_err().contains("Cyclic"));
    }
    #[test]
    fn malformed_inventory_and_nonrigid_transforms_cannot_silently_drop_or_distort_bodies() {
        let mut t = tree();
        t.components[0].children[0].component = 999;
        let bodies = BTreeMap::from([("arbitrary".into(), tetra().checked_triangles().unwrap())]);
        assert!(t.place(&bodies, 1).is_err());
        let mut t = tree();
        t.components[0].children[0].transform[0][0] = 2.;
        assert!(t.place(&bodies, 1).is_err());
        let mut t = tree();
        t.pending_moves.push(json!(1));
        assert!(t.definitions().is_err());
        let mut t = tree();
        t.components[1].bodies.push("arbitrary".into());
        assert!(t.definitions().is_err());
    }
    #[test]
    fn every_repeated_placement_counts_toward_the_unchanged_family_limit() {
        let mut t = tree();
        t.components[0].children.push(Placement { component: 1, transform: IDENTITY });
        let body = vec![[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]; MAX_BUILT / 2 + 1];
        // mesh's per-piece cap is smaller, so use five legal pieces per placement.
        t.components[1].bodies = (0..5).map(|i| i.to_string()).collect();
        let bodies = (0..5).map(|i| (i.to_string(), body[..50000].to_vec())).collect();
        assert!(t.place(&bodies, 10).unwrap_err().contains("400000-triangle"));
    }
}
