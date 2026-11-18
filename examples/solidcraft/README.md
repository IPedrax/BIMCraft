# Original bridge showcase sources

These `.sc` files are source models, not static meshes. Each adjacent mapping is an explicit author choice;
none of these parameter names is recognized specially by BIMCraft Rust or the publisher.

| Source | Instance examples | Type examples |
|---|---|---|
| DrawerBase | width, drawer count, toe kick, hardware switches | height, depth, construction dimensions |
| TwoDoorBase | width, shelf count, fronts/hardware visibility | height, depth, construction dimensions |
| Window | Width, Height, LiteCount, TransomHeight | FrameWidth, FrameDepth, MullionWidth |
| Door | Width, Height, LowerPanelHeight, HandleHeight | FrameWidth, FrameDepth, StileWidth |
| Light | FixtureWidth, CageHeight, Drop, CandleCount | BarWidth, CandleHeight |

See [the workflow and limitations](../../docs/solidcraft-bridge.md). The demo script creates separate before,
after, Undo and Redo projects and screenshots. The gallery's views fit each model independently; they are
not a common-scale size comparison. Window glazing is shown with BC's category color, not translated glass.
The decorative light carries geometry, not an electrical or lighting simulation definition.

All five designs are independently authored study content. No Autodesk families, templates, icons or model
contents are included. Cabinet study lineage: [SDH Woodwork fork](https://github.com/drsurfbum/solidcraft-woodwork).
