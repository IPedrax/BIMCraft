//! The two profiles of one app: BIMCraft's own, which has no ribbon and keeps its tools in a palette at the window's left, and Rev's,
//! the Revit-shaped one, whose tools are a ribbon across the top. Both draw the same table (`tools::TOOLS`); they differ only in how
//! it looks, which is the rule the two front ends keep.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    #[default]
    Bimcraft,
    Rev,
}
