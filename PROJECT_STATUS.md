# SolverPK — estado técnico y traspaso (2026-11-19)

> **Fuente de verdad para continuar el desarrollo.** Leer este archivo antes de modificar la arquitectura, los checkpoints o los benchmarks. El plan priorizado está en [ROADMAP.md](ROADMAP.md).

## 1. Resumen ejecutivo

SolverPK es un proyecto de investigación de póker Texas Hold'em en **Rust sin crates externos**, con tres líneas distintas que no deben confundirse:

| Componente | Archivo principal | Agente | Estado |
|---|---|---|---|
| Heads-Up original | `src/main.rs`, `src/selfplay.rs`, `src/evaluator.rs` | Actor-critic **tabular**, 640 estados/3 acciones | Funciona; checkpoints y pruebas propios, **no** 6-max |
| Arena 6-max | `src/sixmax.rs` | Actor-critic **tabular**, 19.440 estados/6 acciones | Funciona en abstracción no-limit; requiere conformance externo |
| Neuronal 6-max | `src/neural.rs` + `src/sixmax.rs` | MLP **neuronal real** 64→32→6, entrenamiento por backpropagation | **100 millones de manos nuevas de self-play completadas** |
| Ingesta PHH | `src/phh.rs`, `src/imitation.rs`, `src/phh_compare.rs`, `src/phh_eval.rs` | Parser, replay histórico y modelo tabular por imitación | 10.000 manos de Pluribus importadas; no es un solver GTO |

La red neuronal **no existía** al comienzo del proyecto: los primeros checkpoints eran tablas. El entrenamiento de 100M pertenece exclusivamente al modelo neuronal 6-max y se hizo con **manos sintéticas generadas por el simulador**, no con 100M de manos humanas descargadas.

**No existe aún:** solver GTO verificado, CFR+ completo, modelo profundo de millones de parámetros, entrenamiento ROCm, rango probabilístico real de 1.326 combinaciones, motor avalado por pruebas independientes ni entrenamiento de los 21,6M registros HandHQ.

## 2. Arquitectura neuronal comprobada

- `Net`: MLP `f32` de 64 entradas, 32 neuronas ocultas ReLU y 6 salidas de política con softmax enmascarado; **2.278 pesos y sesgos entrenables** (más un promedio de recompensa).
- Acciones abstractas: fold, check/call y cuatro objetivos raise-to (aprox. 33 %, 75 %, 100 % del bote, all-in). No controla libremente todos los tamaños legales del NLHE.
- Entrenamiento: REINFORCE de recompensa terminal y baseline escalar móvil; backpropagation implementada manualmente; no hay optimizador Adam ni critic neuronal.
- La misma red juega **los seis asientos en self-play**; no hay liga neuronal de rivales independientes ni evaluación de explotabilidad.
- Representación actual: cartas propias, board visible, stacks, precio de igualar, bote, ciegas/posición, cantidad de jugadores, textura pública, presión resumida y rangos **heurísticos de tres clases** actualizados por acciones públicas. No recibe cartas privadas del rival ni cartas futuras del board.
- Distilación opcional de la política tabular mediante entropía cruzada. Checkpoint neuronal `SPKNN001`; tabular 6-max `SPK6MAX3`. Formatos **no intercambiables**.
- Entrenamiento ejecutado en **CPU de GitHub Actions**. No hay GPU ni ROCm habilitados.

La implementación principal está en `src/neural.rs` (inferencia, gradientes, guardado/carga), con codificación de observaciones, partidas de aprendizaje y evaluación congelada en `src/sixmax.rs`.

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
- Los artefactos de Actions tienen **retención de 30 días** en los workflows actuales: archivarlos en almacenamiento persistente antes de su expiración. Los checkpoints **no forman parte del árbol de Git**.

Comandos principales:

```bash
cargo test --all-targets
cargo build --release

# Entrenamiento neuronal desde cero
cargo run --release -- --sixmax-neural-train --hands 20000 --seed 42 --checkpoint neural.bin

# Continuar sobre pesos existentes
cargo run --release -- --sixmax-neural-train --resume --hands 10000000 --seed 810211 --learning-rate 0.00060 --checkpoint neural.bin

# Frente al profesor tabular congelado
cargo run --release -- --sixmax-neural-eval --hands 120000 --seed 944104 --checkpoint neural.bin --tabular scratch.bin

# Frente al imitador PHH congelado
cargo run --release -- --sixmax-neural-eval --hands 120000 --seed 944104 --checkpoint neural.bin --tabular scratch.bin --phh-model pluribus-imitation.bin
```

Nota: los comandos de evaluación requieren descargar los checkpoints de referencia, no incluidos en la instalación de código. Para reproducir **exactamente** el resultado publicado hay que usar el commit y los archivos/hash mencionados arriba, no asumir que la rama `main` futura es idéntica.

Flujo de datos: `PHH -> phh.rs -> SPKOBS02 -> imitation.rs -> SPKIMT01 (oponente)`. En paralelo: `simulador 6-max -> observación pública de 64D -> neural.rs -> acción -> recompensa terminal -> gradientes -> SPKNN001`.

## 5. Riesgos y deuda técnica observable

1. **Reglas sin validación independiente:** los tests internos no equivalen a conformidad completa de NLHE 6-max; falta comparar transiciones, apuestas mínimas, short raises, all-ins, botes laterales y reparto de odd chips contra otra implementación.
2. **Generalización insuficientemente medida:** la evaluación principal usa solo dos tipos de rival; un mismo agente juega los seis asientos durante self-play; no se midió exploitability ni desempeño contra una población variada.
3. **Estado parcialmente observado pero muy comprimido:** 64 features no codifican el historial ordenado completo; rangos de rival en solo tres clases y no 169 clases/1.326 combos. La red tiene ~2,3k parámetros, no cientos de miles.
4. **REINFORCE de alta varianza:** recompensa final, baseline global y actualizaciones online; no hay critic neuronal, GAE/PPO, mini-batches, entrenamiento con réplicas ni rollback a la política campeona.
5. **Entrenamiento no íntegramente retomable:** el checkpoint guarda pesos, baseline y steps, pero no el RNG y estado completo de optimizador/experimento; no hay manifiesto versionado con datos y métricas para seleccionar el mejor modelo.
6. **Abstracción de apuestas:** cuatro tamaños fijos, no todas las apuestas posibles; la política de PHH aplana distintos tamaños históricos a una etiqueta bet/raise.
7. **Sin aceleración GPU:** no hay kernel HIP/ROCm/Vulkan, ni multiactor CPU realmente paralelo; throughput reportado corresponde a runner CI.
8. **Cobertura de datos históricos incompleta:** Pluribus 10k importadas; HandHQ 21,6M y ACPC completo pendientes. Los historiales NO suministran respuestas contrafactuales a acciones distintas de las históricas.

**Prioridades y criterios de aceptación:** [ROADMAP.md](ROADMAP.md).

## 6. Instrucción para el siguiente agente

Antes de lanzar entrenamiento adicional: (1) recuperar y verificar SHA-256 del checkpoint 100M; (2) confirmar que el código actual respeta `SPKNN001`; (3) ejecutar suite de regresión; (4) fijar semillas y oponentes de evaluación nuevos; (5) mantener congelados 0M, 10M, 50M, 100M; (6) implementar mejoras **en orden de prioridad** desde [ROADMAP.md](ROADMAP.md); (7) comparar con intervalos de confianza y no promover un modelo solo por el último checkpoint.

Las modificaciones estructurales de entrada/capas requieren **nuevo identificador de checkpoint** y una migración explícita; no reinterpretar pesos binarios antiguos silenciosamente.
