# SolverPK — estado técnico y traspaso (2026-11-19)

> **Estado vigente: Neural V2.** [NEURAL_V2.md](NEURAL_V2.md) documenta el encoder de 128 entradas, aprendizaje monetario sin recortes, actualización congelada por mano, checkpoints reproducibles y liga. SPKNN001 se conserva solo para evaluación.

> **Auditoría externa posterior:** [EXTERNAL_ENGINE_AUDIT.md](EXTERNAL_ENGINE_AUDIT.md) describe la comparación con PokerKit/Treys y las correcciones de controles de caja. [TABULAR_10M_CORRECTED.md](TABULAR_10M_CORRECTED.md) y [TABULAR_20M_CORRECTED.md](TABULAR_20M_CORRECTED.md) conservan los entrenamientos tabulares posteriores; no son entrenamientos neuronales V2.

> **Actualización posterior a los 100M:** [AUDIT_SIXMAX.md](AUDIT_SIXMAX.md) contiene las correcciones verificadas de reglas/rake, la reevaluación congelada y las limitaciones estratégicas detectadas. Las tablas de experimentos anteriores de este archivo son históricas y no representan resultados después de rake.

> **Fuente de verdad para continuar el desarrollo.** Leer este archivo antes de modificar la arquitectura, los checkpoints o los benchmarks. El plan priorizado está en [ROADMAP.md](ROADMAP.md).

## 1. Resumen ejecutivo

SolverPK es un proyecto de investigación de póker Texas Hold'em en **Rust sin crates externos**, con cuatro componentes distintos que no deben confundirse:

| Componente | Archivo principal | Agente | Estado |
|---|---|---|---|
| Heads-Up original | `src/main.rs`, `src/selfplay.rs`, `src/evaluator.rs` | Actor-critic **tabular**, 640 estados/3 acciones | Funciona; checkpoints y pruebas propios, **no** 6-max |
| Arena 6-max | `src/sixmax.rs` | Actor-critic **tabular**, 19.440 estados/6 acciones | Comparación externa reproducible; alcance y diferencias de reglas en EXTERNAL_ENGINE_AUDIT.md |
| Neuronal 6-max | `src/neural.rs` + `src/sixmax.rs` | MLP **128→32→6 V2**, entrenamiento por backpropagation | V2 validada; **100M pertenecen al modelo histórico V1**, conservado |
| Ingesta PHH | `src/phh.rs`, `src/imitation.rs`, `src/phh_compare.rs`, `src/phh_eval.rs` | Parser, replay histórico y modelo tabular por imitación | 10.000 manos de Pluribus importadas; no es un solver GTO |

La red neuronal **no existía** al comienzo del proyecto: los primeros checkpoints eran tablas. El entrenamiento de 100M pertenece exclusivamente al modelo neuronal 6-max y se hizo con **manos sintéticas generadas por el simulador**, no con 100M de manos humanas descargadas.

**No existe aún:** solver GTO verificado, CFR+ completo, modelo profundo de millones de parámetros, entrenamiento ROCm, rango probabilístico real de 1.326 combinaciones, certificación externa de todas las reglas NLHE ni entrenamiento de los 21,6M registros HandHQ.

## 2. Arquitectura neuronal vigente

- `Net`: MLP `f32` de **128 entradas, 32 ReLU y 6 salidas**, 4.326 parámetros; acumulación de gradiente en f64.
- Entrada: dos planos de 52 cartas (propias y board visible) y 24 cantidades públicas. Conserva rango y palo conjuntamente, sin cartas futuras ni ajenas; ignora el orden de las cartas dentro de cada plano.
- Acciones abstractas: fold, check/call y cuatro objetivos raise-to. Se conserva el motor económico auditado y rake explícito.
- Recompensa terminal: neto después de rake dividido por 100 BB, sin recorte de retorno o ventaja. Cada mano usa pesos/baseline anteriores y aplica una sola actualización; cada jugador aporta su propio retorno. La proyección de parámetros a [−8,8] permanece.
- La red comparte parámetros entre los seis asientos durante self-play. No hay critic neuronal, Adam, PPO ni garantía de convergencia a equilibrio.
- `SPKNN002`: dimensiones/versiones, pesos, baseline, contadores, segmentos de entrenamiento y checksum. Resume continúa índices absolutos de cada seed; guardar es atómico y una corrida nueva no reemplaza archivos existentes.
- `SPKNN001`: 64 entradas y 2.278 parámetros históricos. Inferencia original preservada; entrenamiento/destilación rechazados. No existe migración silenciosa.
- Liga congelada: cinco referencias heurísticas y rivales neuronales V1/V2, tabular y PHH opcionales. Detecta solapamiento conocido de repartos; la procedencia histórica desconocida se etiqueta explícitamente.
- Todo el núcleo continúa en Rust stdlib y CPU. Rangos de rivales de tres clases e historial resumido siguen siendo limitaciones.

Módulos: `neural.rs` (red, aprendizaje y persistencia), `sixmax_observation.rs` (encoders), `sixmax.rs` (rollouts/CLI) y `sixmax_league.rs` (evaluación). Evidencia, comandos y resultados: [NEURAL_V2.md](NEURAL_V2.md).

## 3. Cronología del aprendizaje

| Fase | Entrenamiento | Resultado verificable |
|---|---|---|
| Heads-Up inicial | 10M + continuación a 20M, política tabular | 20M no mejoró de forma monótona frente a 10M en duelo congelado |
| PHH Pluribus | 10.000 manos / 91.356 decisiones | Ingesta real, validación y modelo de imitación; **no** Pluribus original |
| 6-max tabular | 100k y 1M de manos sintéticas | Arena multijugador y evaluación con bots aleatorios/PHH; semillas y asientos rotados |
| Primer MLP | 20k self-play | Funciona el gradiente, pero perdió fuertemente contra el tabular |
| Distilación neuronal | 1M manos simuladas del profesor congelado | ~9,3M actualizaciones; rendimiento aproximadamente comparable al profesor |
| **Continuación neuronal** | **10 × 10M = 100M manos de self-play adicionales** | **Completada satisfactoriamente** |

### 3.1 Evidencia del experimento 100M

- **Workflow:** [Neural 100 million 6max self-play hands](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44564068762).
- **Run ID:** `44564068762`; commit ejecutado `0bca3c95a31149d8cb246dd89afd92cc5ffc8170`.
- Estado: `completed / success`. Los 10 pasos de 10M, tests (`44 passed`), validaciones y verificación de conteo terminaron correctamente.
- Checkpoint inicial: [distilación 1M](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44562688853), artifact `sixmax-neural-million`.
- Profesores/oponentes fijos: `scratch.bin` de [ablation 6-max](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44550178708) y `pluribus-imitation.bin` de [importador 10k](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44541033820).
- Se generaron los diez checkpoints `checkpoints/neural-{10,20,...,100}m.bin` y se verificó `CONFIRMED_100M_HANDS=100000000`.
- Actualizaciones totales en el checkpoint final: **1.163.673.406**. Esta cifra incluye las actualizaciones del entrenamiento inicial; no equivale al número de manos.
- Velocidad observada por bloque: **23.728–26.104 manos/s**, según el runner; no es un benchmark de hardware local ni GPU.
- SHA-256 checkpoint inicial: `b7fbe0cfa332b1ab8373a1944257ce01ebf01b4b27a5809f96044fc1f4035ec8`.
- SHA-256 checkpoint final 100M: `a712525d815872286eb960f330b5f585e4218fef627f7276b2e2e1adb08a8ac4`.

**Learning rates (por bloque consecutivo de 10M):**

| Manos acumuladas nuevas | LR | Manos acumuladas nuevas | LR |
|---:|---:|---:|---:|
| 10M | 0,00060 | 60M | 0,00019 |
| 20M | 0,00048 | 70M | 0,00015 |
| 30M | 0,00038 | 80M | 0,00012 |
| 40M | 0,00030 | 90M | 0,000095 |
| 50M | 0,00024 | 100M | 0,000075 |

### 3.2 Evaluación congelada y qué significa

Mismas semillas de evaluación `944104`, **120.000 manos por adversario y checkpoint**, seis posiciones de botón por grupo de reparto, sin entrenamiento durante la evaluación. Resultados del **asiento 0 candidato** en bb/100 en la abstracción sin rake:

| Checkpoint | vs. 5 copias del profesor tabular | vs. 5 copias de imitador PHH |
|---|---:|---:|
| Inicial (tras 1M de distilación) | −22,332 ±25,077 | +112,355 ±15,233 |
| +10M self-play | +24,149 ±8,568 | +29,530 ±6,015 |
| +50M self-play | +36,948 ±13,463 | +44,428 ±8,284 |
| **+100M self-play** | **+60,114 ±14,417** | **+47,346 ±8,090** |

Los márgenes son **intervalos aproximados del 95 %** basados en grupos de seis repartos, no en semillas de entrenamiento independientes.

**Lectura correcta:** se observa una mejora clara frente al profesor tabular. La política final sigue ganando contra el imitador PHH, pero su resultado **es mucho menor que el del checkpoint inicial** contra ese mismo oponente. Más entrenamiento no implica una mejora universal; hay señales de dependencia del rival, olvido de estrategias anteriores o especialización en self-play. Todavía falta un benchmark verdaderamente externo.

**Nunca interpretar estas cifras como winrate esperado ante profesionales, dinero real, exploitability ni GTO.** El «PHH opponent» es una tabla simplificada de acciones históricas, no el agente original Pluribus.

## 4. Artefactos, estructura y reproducción

**Archivo descargable en la ejecución 100M:**

- `neural-100m-full-evidence`: checkpoints intermedios y logs de entrenamiento y evaluación.
- `neural-100m-checkpoint-10m`, `neural-100m-checkpoint-50m`, `neural-100m-checkpoint-100m`: hitos independientes.
- Los artefactos de Actions tienen **retención de 30 días** en los workflows actuales: archivarlos en almacenamiento persistente antes de su expiración. Los checkpoints intermedios históricos no están en Git; el V1 final 100M y los seis pesos del piloto V2 se conservan en [audit/2026-11-19/neural-v2/models](audit/2026-11-19/neural-v2/models).

Comandos principales:

```bash
cargo test --all-targets
cargo build --release

# Entrenamiento neuronal V2 desde cero (ruta nueva)
cargo run --release -- --sixmax-neural-train --rake-bps 0 --hands 20000 --seed 42 --checkpoint neural.bin

# Continuar solo pesos SPKNN002; con igual seed se continúa el índice de reparto
cargo run --release -- --sixmax-neural-train --rake-bps 0 --resume --hands 10000000 --seed 810211 --learning-rate 0.00060 --checkpoint neural.bin

# Frente al profesor tabular congelado
cargo run --release -- --sixmax-neural-eval --rake-bps 0 --hands 120000 --seed 944104 --checkpoint neural.bin --tabular scratch.bin

# Frente al imitador PHH congelado
cargo run --release -- --sixmax-neural-eval --rake-bps 0 --hands 120000 --seed 944104 --checkpoint neural.bin --tabular scratch.bin --phh-model pluribus-imitation.bin
```

Nota: los comandos de evaluación requieren descargar los checkpoints de referencia, no incluidos en la instalación de código. Para reproducir **exactamente** el resultado publicado hay que usar el commit y los archivos/hash mencionados arriba, no asumir que la rama `main` futura es idéntica.

Flujo de datos: `PHH -> phh.rs -> SPKOBS02 -> imitation.rs -> SPKIMT01 (oponente)`. En paralelo: `simulador 6-max -> observación pública de 128D -> neural.rs -> acción -> neto/100BB -> gradiente congelado por mano -> SPKNN002`.

## 5. Limitaciones vigentes

1. **Generalización débil:** el piloto V2 de 100k manos por inicialización mejoró contra uniforme legal y empeoró sus medias frente a los otros siete rivales. No se eligió campeón. Corregir el cálculo no garantiza fuerza estratégica.
2. **Referencias limitadas:** las heurísticas no están validadas competitivamente; PHH es un imitador de 960 estados/3 acciones, no Pluribus. La procedencia de rivales históricos y profesores tabulares no permite certificar un holdout completo.
3. **Información resumida:** cartas exactas visibles, pero sin historial ordenado de apuestas ni rangos completos de 1.326 combinaciones; MLP pequeña.
4. **Varianza y dinámica de self-play:** baseline global y retorno terminal; no hay critic, población entrenable de rivales ni selección validada de checkpoints. El gradiente corregido no prueba equilibrio ni una mejora universal.
5. **Reproducibilidad acotada:** resume exacto con el mismo binario/configuración y segmentos registrados. La identidad/datos completos del profesor histórico y un scheduler general todavía no se serializan; el commit y SHA-256 se guardan en los manifiestos de evidencia.
6. **Motor y variantes:** liquidación contrastada y regresiones amplias; falta conformidad completa de transiciones con otro motor. HU antiguo sigue separado sin rake.
7. **Ejecución:** CPU secuencial; sin ROCm/Vulkan ni multiactor paralelo. PHH Pluribus 10k importado, HandHQ/ACPC completos pendientes.

Prioridades y criterios: [ROADMAP.md](ROADMAP.md).

## 6. Instrucción para el siguiente agente

Leer [NEURAL_V2.md](NEURAL_V2.md) y su JSON de evidencia antes de continuar. Para entrenar: usar un SPKNN002 o una ruta nueva, fijar seed/LR/rake y ejecutar las regresiones; para comparar con 100M: cargar el SPKNN001 congelado y verificar su SHA. No reentrenar ni reinterpretar V1.

Separar selección/entrenamiento/test y conservar resultados de todos los rivales. No promocionar automáticamente el último checkpoint: el piloto actual evidencia especialización contra uniforme. Priorizar entrenamiento con oponentes diversos y control de varianza antes de otra campaña masiva.

Cambios posteriores de observaciones/arquitectura necesitan otro identificador y compatibilidad explícita. Las métricas históricas anteriores mantienen su commit, motor y rake originales.
