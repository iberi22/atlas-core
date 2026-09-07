# atlas-node — wrapper Node/Passenger (ADR-004, REQ-F-021)

Wrapper delgado sin dependencias: espejo JS puro del contrato `atlas-wasm`
con conmutacion al `.wasm` cuando el artefacto existe. La persistencia vive
en Node (`better-sqlite3`, declarada pero NO instalada en este repo).

## Contrato (igual que `wasm/src/lib.rs`)

- `reachable(start, edges_json) -> '["a","b"]'` o `'{"error":"..."}'`.
  `edges_json = [[child,parent],...]`, ancestros ordenados.
- `topo_order(nodes_json, edges_json) -> '["a",...]'` o `'{"error":"..."}'`.
  Padres primero, desempate lexicografico, ciclo = `cycle detected among: …`.

HTTP:

- `POST /traverse {"op":"reachable","start":"c","edges":[["c","b"],["b","a"]]}` → `["a","b"]`
- `POST /traverse {"op":"topo","nodes":["a","b","c"],"edges":[...]}` → orden
- `GET /healthz` → `{"ok":true,"backend":"js"|"wasm"}`
- `edges`/`nodes` aceptan array o string JSON. Errores siempre `{"error":"…"}`.

## Construir el .wasm (en maquina con red + toolchain, NO en el servidor)

Opcion A (recomendada, genera pegamento Node):

```sh
cd wasm
wasm-pack build --target nodejs --out-dir ../node/pkg
# produce ../node/pkg/atlas_wasm.js + atlas_wasm_bg.wasm
```

Opcion B (offline, solo std + wasm-bindgen ya en vendor):

```sh
rustup target add wasm32-unknown-unknown   # una vez, con red
cd wasm
cargo build --target wasm32-unknown-unknown --release
wasm-bindgen --target nodejs \
  --out-dir ../node/pkg \
  ../target/wasm32-unknown-unknown/release/atlas_wasm.wasm
```

Sin artefacto, `app.js` usa el espejo JS (byte-identico segun fixtures de
`wasm/tests/parity.rs`). Con `node/pkg/atlas_wasm.js` presente, conmuta solo
al wasm via `require` (= WebAssembly API por debajo). Nota: el `.wasm` crudo
de wasm-bindgen NO se puede instanciar directo con `WebAssembly.Instance`
(necesita su JS de pegamento); por eso el orden de carga es JS-bindgen
primero, crudo despues, espejo JS como fallback.

## Desplegar en CPanel (Passenger)

1. En tu maquina: construye el wasm (arriba) y corre `npm install --omit=dev`
   para materializar `better-sqlite3` (aqui NO se instala: red medida).
2. Sube `node/` completo (`app.js`, `package.json`, `pkg/`, `node_modules/`)
   al `Application root` de "Setup Node.js App".
3. Application startup file: `app.js`. Version Node ≥ 18.
4. Passenger arranca con `PORT` inyectado; `app.js` escucha `process.env.PORT`
   tambien cuando se requiere (rama Passenger) como con `node app.js`.
5. Verifica: `GET /healthz` → `{"ok":true,"backend":"wasm"}` y
   `POST /traverse` con el ejemplo de arriba.

Limite memoria objetivo: < 512 MB (ADR-004).

## Smoke (sin instalar nada)

```sh
cd node
npm run smoke   # falla con mensaje claro si falta better-sqlite3
node --check app.js
```
