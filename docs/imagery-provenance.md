# Aerial imagery: where the ideas and the data come from

BIMCraft lays aerial photographs over the terrain (`fetch_imagery`), draws them under plans, shows what each is owed, and lets the
user line them up with the map (`shift_imagery`). This is the record of what that rests on: the programs whose behaviour was read for
ideas, and the services the photographs come from, with what is known of their terms and what is not. It is the counterpart of
[rvt-provenance.md](rvt-provenance.md), shorter because no code was ported from anywhere. [CLAUDE.md](../CLAUDE.md)'s clean-room rule
points here.

The agent that wrote this is not a lawyer and makes no claim about whether any use below is lawful in any country or allowed by a
service's terms. The sandbox it worked in could not reach any of these services or open most of the pages named below; every address was
tried on the project owner's machine (2026-11-03 and 2026-11-04), and the terms were read only where a service's own capabilities
document, which the owner ran and pasted, says them. **Before shipping a build that fetches imagery, read each service's own terms**
(the rows below say which pages were not read) and fill in the last column.

## Read for behaviour only; nothing copied

None of the following is in BIMCraft as code, text, icons, data or file formats. What was taken is an idea that every imagery program
has, described in BIMCraft's own words and written afresh against public specifications (the Web Mercator tile scheme, WMTS, PNG).

| Read | Licence | What the idea was | Where it went |
|---|---|---|---|
| JOSM, its imagery layers | GPL-2.0-or-later | A photograph is a layer under the data, and it can be shifted by an offset to line it up with the map; the layer's attribution is shown with it. | `shift_imagery` and the Site tab's Align Imagery; `imagery::credits` and the credit plate; the photograph under the plan. |
| Blender, its image texture node (Flat projection, extension mode "Extend") | GPL-2.0-or-later (the Manual is CC-BY-SA 4.0 and was paraphrased, never pasted) | A picture can be laid over a surface by where the surface is (x and y), not across its own plane, and held at its edge rather than repeated. | `Texture.planar`, the clamp in `Sampler`, the card's shader branch. |
| BlenderGIS | GPL-3.0 | A raster's own georeferencing gives its place on a mesh; a mosaic too big for one texture is a reason to choose the zoom from a pixel budget. | `imagery::plan`, `imagery::warp`, the world file `imagery warp` writes. |
| Bonsai (BlenderBIM) and the IFC4 texture entities | GPL-3.0; IFC4 is ISO 16739 | How IFC carries an image on a surface (`IfcImageTexture`, `IfcIndexedTriangleTextureMap`), and that viewers honour it unevenly. | Nothing yet: imagery in the IFC export is not built. |

The sandbox could not open docs.blender.org, the buildingSMART pages or the Bonsai documentation; what is above was read in search
snippets. Names of IFC entities are to be checked against the public IFC4.3 specification before anything is built on them.

## The services

BIMCraft's table of sources is `imagery::SOURCES` (id, address template, finest zoom, rectangle, credit, terms); `net::Allow` lets the
fetcher reach each source's addresses and no others. A service is in the table only if its terms, as far as they were read, allow
fetching its tiles into a project. Bing, Esri, Maxar, Mapbox and Google are left out because, as far as the research found, theirs do not.

| id | Service | What was read | Credit used | Not read | Verified on |
|---|---|---|---|---|---|
| `flanders` | Digitaal Vlaanderen, Orthofotomozaïek, middenschalig, winteropnamen, kleur, meest recent: `geo.api.vlaanderen.be/OMWRGBMRVL/wmts`, asked by query (WMTS GetTile), PNG only, zoom 0 to 21 | Its capabilities document: "Het gebruik van de service is kosteloos" (free of charge); tile matrix set `GoogleMapsVL` is the ordinary Web Mercator pyramid; no tile addresses of its own. | "(c) Digitaal Vlaanderen - Orthofotomozaiek Vlaanderen", the wording the owner was told Digitaal Vlaanderen recommends | The usage-right page, "gebruiksrecht en privacyverklaring geografische webdiensten" (its address is the source's `terms`) | |
| `pdok` | PDOK Luchtfoto, `Actueel_orthoHR`: `service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0/.../EPSG:3857/{z}/{x}/{y}.jpeg`, zoom to 21 | Its capabilities document: fees and constraints "none". CC BY 4.0 is from the research, not from a page that was opened. | "Luchtfoto Actueel_orthoHR: Landelijke Voorziening Beeldmateriaal, PDOK, CC BY 4.0" | PDOK's dataset page (the licence and the wording it asks for) | |
| `usgs` | USGS Imagery Only, `basemap.nationalmap.gov/.../USGSImageryOnly/MapServer/tile/{z}/{y}/{x}`, to zoom 16 (about 2 m a pixel); its document lists 24 levels but 404 begins at 17 | Its capabilities document states no terms. Public domain with a request for credit is from the research. | "Imagery courtesy of the U.S. Geological Survey, The National Map" | USGS's data-citation page | |
| `eox2016` | EOX Sentinel-2 cloudless, layer `s2cloudless_3857`, `tiles.maps.eox.at/wmts/1.0.0/.../GoogleMapsCompatible/{z}/{y}/{x}.jpg`, to zoom 14 (about 6 m a pixel) | Its capabilities document: attribution is mandatory and carries links, and "additional restrictions may apply for individual layers as indicated in the respective abstract". | "EOxCloudless https://cloudless.eox.at by EOX IT Services GmbH (Contains modified Copernicus Sentinel data 2016), CC BY 4.0" (the links are text: a plate cannot link) | The layer's abstract. **Whether this layer is the 2016 mosaic is unconfirmed**; the later mosaics are non-commercial share-alike. | |

The elevation and building data have their own credits and terms (OpenStreetMap under the ODbL, Terrarium, Copernicus DEM): see
[geofetch](areas/geofetch.md).

## What the program does with a credit

The credit text travels with the data (the toposurface's `Imagery Credit` parameter), and every place the photograph is shown letters it:
plans, sections, sheets and their PDF, the 3D window and the 3D pictures the core makes ([terrain](areas/terrain.md#the-photograph-in-plans)).
It is text only; a service's address for its terms is the toposurface's `Imagery Terms` and the Site tab's result box. It is not written
into the IFC export.
