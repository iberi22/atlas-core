# ADR-004: SWAL Operational Deterministic Protocol (SODP) — Códigos de Estado, Telemetría y Snippets Canónicos

- **Estado:** Aceptado
- **Fecha:** 2026-09-08
- **Autores:** Belal & Antigravity (SWAL Architecture Core)
- **Aplica a:** Ecosistema completo (`atlas-core`, `gestalt`, `swal-agent-runner`, `swal-agent-mobile`, `xavier`)

## Contexto y Problema

El ecosistema SWAL delega trabajo a agentes autónomos heterogéneos (Jules, Claude Code, OpenCode CLI, Hermes). La orquestación previa basada en descripciones en lenguaje natural libre presenta tres deficiencias críticas:
1. **Alucinaciones y Deriva Contextual:** Los agentes interpretan subjetivamente criterios de finalización ("DoD"), asumen que advertencias son éxitos y cometen inconsistencias de estado.
2. **Consumo Excesivo de Tokens:** Mensajes explicativos en prosa saturan la ventana de contexto y los límites de cuota (TPM/RPM).
3. **Imposibilidad de Validación Determinista en Base de Datos:** Los estados no estructurados impiden que SQLite / Postgres / IndexedDB validen transiciones mediante restricciones formales (`FOREIGN KEY`, triggers) o máquinas de estado finitas (FSM).

## Decisión

Se adopta el **Protocolo Operativo Determinista de SWAL (SODP / Status Code Protocol)**:

### 1. Taxonomía de Rangos Numéricos de 3 y 4 Dígitos
- `100 - 199`: Ciclo de vida de tareas de agentes (`100` Backlog, `101` Ready, `102` Dispatched, `103` Running, `110` Blocked Dep, `111` Blocked Auth, `120` In Review, `130` Completed, `140` Failed).
- `200 - 299`: Git, VFS y File Islands (`200` Island Locked, `201` Branch Created, `202` Tree Dirty, `203` Committed, `204` Pushed, `210` PR Opened, `212` Merged, `220` Conflict, `221` Island Breach).
- `300 - 399`: Build, Verificación y CI/CD (`300` Queued, `301` Building, `302` Build OK, `303` Tests Pass, `304` Staging, `305` Prod, `310` Build Fail, `311` Test Fail, `312` E2E Fail, `313` Cov Drop).
- `400 - 499`: Infraestructura, Nodos y Red Mesh (`400` Online, `401` Degraded, `402` Standby/Sleep, `410` Mesh Synced, `411` Offline, `420` Xavier Ready, `421` Sync Pending).
- `500 - 599`: Incidencias Críticas y Escapes (`500` SEV-0 Critical Showstopper, `501` SEV-1 Blocker, `502` SEV-2 Degraded, `510` Secret Leak Detected, `520` OOM Crash).
- `800 - 899`: Alineación con el Ledger de Features (`801` Draft, `802` Beta, `803` Promoted).
- `900 - 999`: Catálogo de Snippets Canónicos / Arquetipos de Código (`SNP-901` a `SNP-905`).

### 2. Máquina de Estados Finitos (FSM)
Toda transición debe pertenecer a la matriz de transiciones permitidas. Las transiciones a estados de éxito (`130`, `302`, `303`) exigen carga de evidencia atómica (exit code, logs resumidos, duración).

### 3. Persistencia Relacional
Se define en SQLite (`store.rs`), Postgres (`xavier`) e IndexedDB (`swal-agent-runner`, `swal-agent-mobile`):
- `status_catalog (code INT PRIMARY KEY, slug TEXT, category TEXT, sev INT, description TEXT)`
- `state_transitions (from_code INT, to_code INT, requires_evidence INT)`
- `task_incidents (id TEXT, task_id TEXT, code INT, sev INT, metadata_json TEXT, resolved INT)`

### 4. Soporte Multi-Lenguaje
Se estandarizan enums nativos fuertemente tipados:
- Rust: `apps/atlas-core/src/protocol/status.rs`
- Dart / Flutter: `apps/swal-agent-mobile/lib/protocol/status.dart`
- TypeScript: `cores/swal-agent-runner/src/types/protocol.ts`

## Consecuencias
- Cero ambigüedad en la telemetría entre agentes.
- Ahorro de más del 85% de tokens en eventos de progreso.
- Filtrado y dashboards inmediatos en dispositivos móviles.
- Rechazo determinista a nivel de base de datos de cualquier intento de salto de estado inválido.
