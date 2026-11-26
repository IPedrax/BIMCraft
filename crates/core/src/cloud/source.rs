//! Where a scan's index keeps its nodes ([`NodeSource`]): views ask for nodes through it, never of a format, so an index of our
//! own (`.bimpc`, `cloud/index.rs`) and any other octree a view reads (COPC, one day EPT) are drawn the same way.

use super::index::{Index, NodePoints};

/// An octree of a scan's points, each node a grid sample of what's below it, as views read it. Every point is in one node; a
/// node's places are from the scan's middle, mm, as a [`super::Cloud`]'s.
pub trait NodeSource: Send + Sync + std::fmt::Debug {
    /// Which index it is: a project holds the same, so another's is never taken for it.
    fn id(&self) -> u128;
    /// The points it holds.
    fn count(&self) -> u64;
    /// How many nodes it has: they're numbered from 0, the root.
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Node `i`'s level (0 the root) and its points; none past the last node.
    fn node(&self, i: usize) -> Option<(u8, u32)>;
    /// Node `i`'s children that hold anything or have children of their own.
    fn children(&self, i: usize) -> Vec<usize>;
    /// Node `i`'s cube from the scan's middle, mm: its lowest corner and how wide it is.
    fn cube(&self, i: usize) -> ([f64; 3], f64);
    /// How far apart a node's points are at `level`, at least, mm.
    fn spacing(&self, level: u8) -> f64;
    /// Node `i`'s points, read now.
    fn load(&self, i: usize) -> Result<NodePoints, String>;
    /// What it takes on disk, bytes.
    fn size(&self) -> u64;
}

impl NodeSource for Index {
    fn id(&self) -> u128 {
        self.header.id
    }

    fn count(&self) -> u64 {
        self.header.count
    }

    fn len(&self) -> usize {
        self.nodes.len()
    }

    fn node(&self, i: usize) -> Option<(u8, u32)> {
        self.nodes.get(i).map(|n| (n.level, n.points))
    }

    fn children(&self, i: usize) -> Vec<usize> {
        Index::children(self, i).collect()
    }

    fn cube(&self, i: usize) -> ([f64; 3], f64) {
        Index::cube(self, i)
    }

    fn spacing(&self, level: u8) -> f64 {
        Index::spacing(self, level)
    }

    fn load(&self, i: usize) -> Result<NodePoints, String> {
        Index::load(self, i)
    }

    fn size(&self) -> u64 {
        Index::size(self)
    }
}
