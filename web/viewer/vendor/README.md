# The viewer's libraries

Kept here, unchanged, so the viewer makes no request outside the server that serves it. Each comes from its npm registry tarball, whose sha512 was checked against the
registry's `dist.integrity` before the files were copied out. Their MIT licences are in `../LICENSES`.

| Package | Tarball | dist.integrity | Files taken |
|---|---|---|---|
| three 0.170.0 | https://registry.npmjs.org/three/-/three-0.170.0.tgz | sha512-FQK+LEpYc0fBD+J8g6oSEyyNzjp+Q7Ks1C568WWaoMRLW+TkNNWmenWeGgJjV105Gd+p/2ql1ZcjYvNiPZBhuQ== | `build/three.module.js`, `examples/jsm/loaders/GLTFLoader.js`, `examples/jsm/utils/BufferGeometryUtils.js` |
| three-mesh-bvh 0.8.3 | https://registry.npmjs.org/three-mesh-bvh/-/three-mesh-bvh-0.8.3.tgz | sha512-4G5lBaF+g2auKX3P0yqx+MJC6oVt6sB5k+CchS6Ob0qvH0YIhuUk1eYr7ktsIpY+albCqE80/FVQGV190PmiAg== | `build/index.module.js` |

A new file the viewer imports from `three/addons/` is copied from the same tarball to the same path under `three/examples/jsm/`.
