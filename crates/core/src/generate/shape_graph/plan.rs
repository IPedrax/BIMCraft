//! Steps 1 and 2: the room program and the relationship graph, then the clusters and groups the layout places.

use std::collections::BTreeMap;

use super::family::{Conn, Family, Place, Use};
use super::fit::{Cl, Fail, Group, R, Rnd};

#[derive(Clone, Debug)]
pub struct Room {
    /// A slug: the room type's key, with `_2`, `_3` when the plan has several of the type.
    pub id: String,
    pub ty: usize,
    /// The net area wanted, m2.
    pub target: f64,
}

/// The room program: how many rooms of each type, adding up to `n`.
pub fn program(fam: &Family, n: i64, rng: &mut Rnd) -> R<Vec<Room>> {
    let types = &fam.types;
    let mut cnt: Vec<i64> = types.iter().map(|t| t.count.0).collect();
    let follow = |cnt: &mut Vec<i64>, start: usize| {
        let mut todo = vec![start];
        while let Some(t) = todo.pop() {
            for (u, ty) in types.iter().enumerate() {
                if let Some((owner, lo, _)) = ty.per
                    && owner == t
                    && lo * cnt[t] > cnt[u]
                {
                    cnt[u] = lo * cnt[t];
                    todo.push(u);
                }
            }
        }
    };
    for t in 0..types.len() {
        follow(&mut cnt, t);
    }
    let can_add = |cnt: &Vec<i64>, t: usize| {
        if cnt[t] >= types[t].count.1 {
            return false;
        }
        types[t].per.is_none_or(|(owner, _, most)| cnt[t] < most * cnt[owner])
    };
    while cnt.iter().sum::<i64>() < n {
        let cands: Vec<usize> = (0..types.len()).filter(|t| can_add(&cnt, *t)).collect();
        let weights: Vec<f64> = cands.iter().map(|t| types[*t].weight).collect();
        let t = cands[rng.weighted(&weights)?];
        cnt[t] += 1;
        follow(&mut cnt, t);
    }
    if cnt.iter().sum::<i64>() != n {
        return Err(Fail("program"));
    }
    let mut rooms = Vec::new();
    for (t, ty) in types.iter().enumerate() {
        for i in 0..cnt[t] {
            let id = if cnt[t] == 1 { ty.key.clone() } else { format!("{}_{}", ty.key, i + 1) };
            rooms.push(Room { id, ty: t, target: ty.area.0 + (ty.area.1 - ty.area.0) * rng.uniform(0.15, 0.6) });
        }
    }
    Ok(rooms)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    Cap0,
    Cap1,
    Cell,
    Band,
    Strip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parent {
    Spine,
    Room(usize),
}

pub struct Graph {
    pub spines: Vec<usize>,
    pub root: Option<usize>,
    pub parent: BTreeMap<usize, Parent>,
    pub ltypes: BTreeMap<usize, Vec<Conn>>,
    /// Children placed inside a room's cell.
    pub strip: Vec<Vec<usize>>,
    /// Children placed in the next cell of the row.
    pub band: Vec<Vec<usize>>,
    pub how: Vec<Option<How>>,
    pub cap1: Option<usize>,
}

/// Every room but the entry room and the spine rooms gets one parent: another room, or the spine (which spine room is settled by
/// the layout).
pub fn graph(fam: &Family, rooms: &[Room], rng: &mut Rnd) -> R<Graph> {
    let n = rooms.len();
    let ty = |i: usize| rooms[i].ty;
    let place = |i: usize| fam.types[ty(i)].place;
    let is_spine = |t: usize| fam.types[t].place == Place::Spine;
    let spines: Vec<usize> = (0..n).filter(|i| place(*i) == Place::Spine).collect();
    if spines.is_empty() {
        return Err(Fail("no spine"));
    }
    let root = fam.entry.as_ref().and_then(|e| (0..n).find(|i| e.rooms.contains(&ty(*i)) && place(*i) == Place::EntryEnd));
    let mut g = Graph {
        spines,
        root,
        parent: BTreeMap::new(),
        ltypes: BTreeMap::new(),
        strip: vec![Vec::new(); n],
        band: vec![Vec::new(); n],
        how: vec![None; n],
        cap1: None,
    };
    let mut placed: Vec<usize> = Vec::new();
    let mut nlinks = vec![0usize; n];
    let mut depth = vec![0usize; n];
    if let Some(r) = root {
        g.how[r] = Some(How::Cap0);
        placed.push(r);
        nlinks[r] = 1;
    }
    let mut pending: Vec<usize> = (0..n).filter(|i| Some(*i) != root && place(*i) != Place::Spine).collect();
    rng.shuffle(&mut pending);
    while !pending.is_empty() {
        let mut progress = false;
        for i in pending.clone() {
            // (weight, parent, link, how)
            let mut cands: Vec<(f64, Parent, usize, How)> = Vec::new();
            for (li, l) in fam.links.iter().enumerate() {
                if l.use_ != Use::Parent || l.to != ty(i) {
                    continue;
                }
                if is_spine(l.from) {
                    cands.push((l.weight, Parent::Spine, li, if place(i) == Place::SpineEnd { How::Cap1 } else { How::Cell }));
                    continue;
                }
                if matches!(place(i), Place::SpineEnd | Place::EntryEnd) {
                    continue;
                }
                for &j in &placed {
                    if ty(j) != l.from || (!l.types.is_empty() && nlinks[j] >= fam.types[ty(j)].max_links) {
                        continue;
                    }
                    if let Some((owner, _, most)) = fam.types[ty(i)].per
                        && owner == ty(j)
                        && g.strip[j].iter().chain(&g.band[j]).filter(|k| ty(**k) == ty(i)).count() as i64 >= most
                    {
                        continue;
                    }
                    let how_j = g.how[j];
                    if place(i) == Place::OffSpine {
                        if (how_j == Some(How::Cell) && g.band[j].len() < 2) || (how_j == Some(How::Band) && g.band[j].is_empty()) {
                            cands.push((l.weight, Parent::Room(j), li, How::Band));
                        }
                    } else if depth[j] < 2 {
                        cands.push((l.weight, Parent::Room(j), li, How::Strip));
                    }
                }
            }
            if cands.is_empty() {
                continue;
            }
            let pick = rng.weighted(&cands.iter().map(|c| c.0).collect::<Vec<_>>())?;
            let link = cands[pick].2;
            let same: Vec<&(f64, Parent, usize, How)> = cands.iter().filter(|c| c.2 == link).collect();
            let load = |p: Parent| match p {
                Parent::Spine => 0,
                Parent::Room(j) => g.strip[j].len() + g.band[j].len(),
            };
            let fewest = same.iter().map(|c| load(c.1)).min().unwrap_or(0);
            let tied: Vec<&&(f64, Parent, usize, How)> = same.iter().filter(|c| load(c.1) == fewest).collect();
            let &&(_, p, _, how) = rng.pick(&tied)?;
            let types = fam.links[link].types.clone();
            let counts = !types.is_empty();
            g.parent.insert(i, p);
            g.how[i] = Some(how);
            placed.push(i);
            if counts {
                nlinks[i] += 1;
            }
            match p {
                Parent::Spine => depth[i] = 0,
                Parent::Room(j) => {
                    if counts {
                        nlinks[j] += 1;
                    }
                    depth[i] = depth[j] + usize::from(how == How::Strip);
                    if how == How::Strip {
                        g.strip[j].push(i);
                    } else {
                        g.band[j].push(i);
                    }
                }
            }
            g.ltypes.insert(i, types);
            pending.retain(|x| *x != i);
            progress = true;
        }
        if !progress {
            return Err(Fail("graph"));
        }
    }
    for (t, ty_t) in fam.types.iter().enumerate() {
        if let Some((owner, lo, _)) = ty_t.per
            && lo > 0
        {
            for j in 0..n {
                if ty(j) == owner && (g.strip[j].iter().chain(&g.band[j]).filter(|k| ty(**k) == t).count() as i64) < lo {
                    return Err(Fail("per"));
                }
            }
        }
    }
    let caps: Vec<usize> = (0..n).filter(|i| g.how[*i] == Some(How::Cap1)).collect();
    if caps.len() > 1 {
        return Err(Fail("two end rooms"));
    }
    g.cap1 = caps.first().copied();
    Ok(g)
}

/// The clusters and groups the layout places: the start end room's cluster, the far end room's, and the groups of cells.
pub fn clusters(g: &Graph) -> (Option<Cl>, Option<Cl>, Vec<Group>) {
    fn mk(g: &Graph, i: usize) -> Cl {
        Cl {
            head: i,
            kids: g.strip[i].iter().map(|k| mk(g, *k)).collect(),
            need: g.ltypes.get(&i).and_then(|t| t.iter().map(|c| c.need()).min()).unwrap_or(0),
            goal: None,
        }
    }
    fn chain(g: &Graph, mut j: usize) -> Vec<usize> {
        let mut out = vec![j];
        while let Some(&next) = g.band[j].first() {
            j = next;
            out.push(j);
        }
        out
    }
    let mut groups = Vec::new();
    for i in 0..g.how.len() {
        if g.how[i] != Some(How::Cell) {
            continue;
        }
        let right = g.band[i].first().map_or_else(Vec::new, |j| chain(g, *j));
        let mut left = g.band[i].get(1).map_or_else(Vec::new, |j| chain(g, *j));
        left.reverse();
        let order: Vec<usize> = left.into_iter().chain([i]).chain(right).collect();
        let last = order.len() - 1;
        groups.push(order.iter().enumerate().map(|(k, j)| (mk(g, *j), k > 0, k < last)).collect());
    }
    (g.root.map(|r| mk(g, r)), g.cap1.map(|c| mk(g, c)), groups)
}
