# SolverPK — plan de mejoras priorizado después del entrenamiento neuronal 100M

**Fecha:** 2026-11-19. **Base comprobada:** [PROJECT_STATUS.md](PROJECT_STATUS.md). No se propone otro entrenamiento masivo hasta superar los criterios P0.

La red actual aprende y gana contra el bot tabular en la simulación, pero todavía no podemos estimar su calidad frente a jugadores competitivos ni su explotabilidad. Este plan prioriza **validez de resultados → calidad estratégica → velocidad**. Cada tarea tiene implementación, prueba y condición explícita para declararla finalizada.

## Resumen de prioridades

| Orden | Mejoras concretas | Por qué primero | Resultado exigido |
|---|---|---|---|
| P0.1 | Motor 6-max auditado independientemente | Evitar entrenar sobre reglas erróneas | Reproducción idéntica de botes, turnos y showdown |
| P0.2 | Liga de adversarios y métricas repetibles | Detectar sobreajuste a dos oponentes | CI, bb/100, rankings de checkpoints y semillas independientes |
| P0.3 | Checkpoints transaccionales y reproducibilidad | No perder experimentos ni seleccionar pesos incorrectos | Estado, hashes, RNG, manifiesto, prueba de resume |
| P0.4 | Control de colapso/explotabilidad del self-play | 100M no mejoró contra todos | Best-checkpoint, alertas de regresión, oponentes congelados |
| P1.1 | Rangos de manos completos | Reemplazar heurística de tres clases | Distribución normalizada de 1.326 combos con blockers |
| P1.2 | Historial de acciones completo | La red actual ve resúmenes, no secuencias | Estado compatible con información disponible por jugador |
| P1.3 | Actor–critic neuronal y arquitectura v2 | Reducir varianza de REINFORCE, ampliar capacidad | Comparación ablation con misma inversión computacional |
| P1.4 | Entrenamiento PHH real a escala | Aprovechar Pluribus/HandHQ/ACPC | Streaming, deduplicado, splits sin filtración |
| P1.5 | Referencias CFR y búsqueda | Medir pérdida de EV, no solo winrate | Exploitability válida en abstracción HU y tests 6-max aproximados |
| P2.1 | Paralelización CPU y batching | Escalar arenas sin cambiar la matemática | Benchmark reproducible y escalabilidad |
| P2.2 | Backend GPU AMD aislado | Aprovechar VRAM/compute cuando sea rentable | Paridad numérica CPU/GPU + throughput útil |
| P2.3 | Red más grande y selección multi-semilla | Aumentar capacidad solo cuando existan datos buenos | Mejora significativa sin degradación del conjunto de rivales |

## P0 — Bloqueantes para afirmar calidad real

### P0.1 Auditoría independiente del motor (primera prioridad)

**Problema:** `src/sixmax.rs` ya maneja ciegas, acciones, raises, all-ins, botes secundarios y splits, pero su exactitud se comprobó principalmente con tests internos. Dos errores que se compensan pueden preservar fichas y producir estrategias incorrectas.

**Cambios:**
- Introducir módulo `rules` independiente del agente, con máquina de estados inmutable/testeable para apuestas y liquidaciones.
- Crear corpus de conformidad: preflop multiway, BB option, apuestas mínimas, raise-to, all-in por menos del mínimo, reopening, secuencias de short raises, side pots anidados, dead chips, tie pots, odd chips, agotamiento de stacks, cartas duplicadas y orden de button.
- Comparar las mismas secuencias y saldos con una implementación de póker diferente y confiable. El replay PHH se usa como fuente adicional, **no** como único oráculo (comparte supuestos con nuestro motor).
- Añadir generación pseudoaleatoria reproducible de secuencias legales; verificar a cada paso conservación de fichas, turnos, acción legal y payout correcto.

**Aceptación:** 100 % de casos conformes sin diferencias no explicadas; ≥100.000 partidas aleatorias con invariantes; tests de regresión para cada bug y comparación cruzada de resultados terminales. Bloquear entrenamiento si falla.

### P0.2 Liga de evaluación verdaderamente fuera de entrenamiento

**Problema:** solo se midió contra profesor tabular y un imitador PHH simplificado, con la política neuronal usando la misma red en los seis asientos de self-play.

**Cambios:**
- Interfaz `Policy` unificada: random legal, tight-aggressive, loose-aggressive, calling station, nits, varias políticas tabulares, PHH, checkpoint neural 0M/10M/50M/100M y versiones futuras.
- Repetir exactamente los repartos bajo rotación de asientos; el rival debe responder **dinámicamente** a las apuestas reales, no reproducir acciones históricas.
- Evaluación congelada sobre semillas de test nunca usadas durante el entrenamiento, grupos de repartos emparejados y poblaciones de rivales aisladas.
- Incorporar métricas: bb/100 por rival, CI 95 % por clusters, resultados por calle/posición, exposición a all-in, frecuencia y sizing de apuestas, calibración y rendimiento multi-semilla. Reportar también el peor rival, no solo el promedio.
- Entrenar al menos 5 inicializaciones independientes para estimar variabilidad **entre modelos**, además de variabilidad **entre manos**.

**Aceptación:** suite determinista reproducible y auditable; controles mirror/clone compatibles con cero; comparación de los cuatro checkpoints y 1M manos por emparejamiento clave; no promocionar un campeón si su mejora depende únicamente de un adversario.

### P0.3 Checkpoints y trazabilidad a prueba de interrupciones

**Problema:** `SPKNN001` guarda pesos, baseline y contador de steps, pero **no** el estado del RNG ni un manifiesto de entrenamiento completo; el guardado es directo al archivo y los artifacts de Actions expiran.

**Cambios:**
- `SPKNN002`: versionado formal de arquitectura, dimensiones, pesos, baseline/value state, RNG, scheduler de learning rate, total de manos, seed original, hash del código y sumas de verificación.
- Escritura a archivo temporal + `fsync` + rename atómico cuando lo permita el sistema de archivos; validación estricta al recuperar.
- Test de reanudación: corrida de N+M manos debe reproducir exactamente la misma política que corrida continua de N+M, con RNG y calendario idénticos.
- Manifiesto Markdown/JSON para cada experimento y checksum SHA-256 del modelo; publicar resultados por commit y no solo por último nombre de archivo.
- Conservar los checkpoints campeones y las métricas en almacenamiento persistente (no depender de 30 días de retención de Actions).

**Aceptación:** restore fallido nunca corrompe el último checkpoint; reanudación bit a bit idéntica para condiciones iguales; script de reproducción documentado.

### P0.4 Evitar regresiones durante self-play

**Problema:** el checkpoint inicial sumaba +112,36 bb/100 frente al imitador PHH y el de 100M +47,35, mientras mejoró ante el profesor. El self-play puede producir estrategias demasiado especializadas.

**Cambios:**
- Liga de adversarios congelados y elección aleatoria controlada de rival por mano.
- Evaluación de cada 1–5M manos con política congelada; guardar `last` y `best` por separado.
- Alertas por caída de rendimiento, KL respecto de política anterior, saturación de logits, falta de diversidad y frecuencia anormal de fold/all-in.
- Mantener replays de partidas problemáticas y probar rollback sin reiniciar de cero.
- Selección de campeón por robustez y límites inferiores de confianza, no por las últimas iteraciones.

**Aceptación:** el pipeline identifica una caída intencional provocada por una estrategia degenerada y conserva el mejor checkpoint sin sobrescribirlo.

## P1 — Mejoras de capacidad estratégica

### P1.1 Rangos verdaderos por combinaciones y blockers

**Diseño:** sustituir `estimated_ranges[[f32;3];6]` por una distribución sobre las **1.326 combinaciones no ordenadas de dos cartas** por jugador. Normalizar tras bloquear cartas propias/públicas; actualizar probabilidades usando contexto, posición, historial y tamaño de apuesta. Opcionalmente comprimir a **169 clases preflop** para acelerar el prefill, sin perder máscaras de combinaciones reales.

**Aceptación:** normalización ≈1, probabilidad cero a cartas bloqueadas y combos duplicados, posterior reproducible, y tests donde rangos estrechos cambien sensatamente la decisión. No usar cartas ocultas para construir la inferencia del agente.

### P1.2 Historial completo y acciones de tamaño variable

**Diseño:** representar cada evento como `(street, asiento, acción, raise-to, stack, pot, timestamp lógico)`; mantener memoria bounded por calle, con máscara para eventos faltantes. Exponer valores proporcionales al bote y stack efectivo, no solo `raise_count`. Conservar un head de tamaños que permita múltiples raise-to legales.

**Aceptación:** dos estados con iguales cartas/pote pero **historias distintas** generan embeddings diferentes; tests de no filtración de futuras cartas y cartas ajenas. La distribución de bet-sizing debe sumar 1 solo en acciones legales.

### P1.3 Evolución neuronal con value head

**Diseño recomendado:** arquitectura v2 con 128–256 dimensiones y 2–3 capas iniciales, embeddings de cartas/posición, codificador de secuencia o agregador de acciones, dos salidas separadas (**policy + value**). Entrenar con actor–critic/A2C o PPO con mini-batches, normalización de ventajas, clipping de gradiente y bonificación entrópica; probar primero en CPU. La abstracción de acciones permanece estable hasta auditar el motor.

**Aceptación:** superar un baseline idéntico en datos/compute; estabilidad en al menos 5 semillas de entrenamiento; latencia, memoria y throughput registrados; métricas contra toda la liga. Un aumento de parámetros sin mejora validada no se considera éxito.

### P1.4 Aprendizaje histórico PHH sin sesgos de imitación

**Diseño:** parser y normalizador en streaming de `HandHQ` (21,6M) y luego ACPC no-limit cuando los archivos completos estén disponibles; deduplicado por huella de mano/sesión y división por sesión o actor para evitar fugas. Separar imitación de comportamiento humano, distilación de solver y aprendizaje de EV. Mapear los tamaños históricos a acciones legales en la nueva abstracción y reportar tasa de descarte.

**Aceptación:** prueba de ingesta completa con conteos y errores por causa; train/test sin manos duplicadas; calibración de política fuera de muestra; tests explícitos de que el agente no ve el futuro ni hole cards rivales.

### P1.5 Aproximación a equilibrio y pérdida de EV

**Diseño:** CFR/CFR+ en Heads-Up y abstracciones pequeñas con valores verificables; para 6-max usar evaluación aproximada de respuesta óptima, búsqueda local o estimadores de NashConv con incertidumbre claramente declarada. Las decisiones de Pluribus histórico **no son etiquetas GTO**.

**Aceptación:** casos de Kuhn/Leduc o endgames discretos resueltos con error conocido; tabla de regret/EV-loss respecto del mismo espacio de acciones; no publicitar exploitability exacta de 6-max completo.

## P2 — Rendimiento, escalado e ingeniería de producción

### P2.1 Arenas multicore Rust, RNG y batching

- Separar `engine`, `observation`, `policy`, `training`, `evaluation` en módulos con interfaces estables.
- Ejecutar partidas en múltiples hilos; semilla por mano independiente de planificación/hilo.
- Combinar gradientes en lotes deterministas o documentar el cambio estadístico; registrar throughput efectivo **con backprop**, no solamente inferencia.
- Perf counters: manos/s, decisiones/s, tiempo de prefill, memoria máxima, colas y CPU.

**Aceptación:** reproducibilidad de la misma semilla y configuración; throughput superior al baseline de ~24–26k manos/s durante entrenamiento 100M en hardware comparable, sin variar reglas y scores de replay.

### P2.2 GPU AMD opcional, no prematuro

- Probar primero batch de inferencia y backprop sobre CPU optimizado.
- Añadir backend HIP/ROCm o Vulkan compute mediante interfaz aislada; comprobar versiones, drivers y compatibilidad **en el hardware real** antes de fijar un framework.
- Mantener el core de reglas y seguridad en Rust. Las bibliotecas de GPU necesarias, si las hay, quedan acotadas detrás de un feature flag.
- Medir transferencias PCIe, ocupación, tamaño de lote y coste real por decisión; una red de ~2,3k parámetros podría ser más rápida en CPU por overhead de GPU.

**Aceptación:** paridad CPU/GPU dentro de tolerancias definidas, benchmarking en lote a igualdad de modelo y datos, mejora de throughput/energía medida y fallback CPU.

### P2.3 Arquitectura escalable y experimentos con selección objetiva

Implementar capacidades 0,5–2M parámetros **solo después** de P1.1–P1.3, de que las entradas tengan suficiente información y de tener una liga válida. Ejecutar ablaciones de rangos, secuencia, red, critic y número de expertos por separado. Evitar entrenamientos masivos sin checkpoints de best-of-run.

**Aceptación:** mejoras replicadas con semillas nuevas, incluyendo oponentes que no participaron del entrenamiento y un reporte reproducible de costes y ganancias.

## Orden de implementación y primer entregable

**Sprint de fiabilidad (primero):**
1. Refactor mínimo para aislar apuestas/liquidación y añadir corpus de conformidad independiente.
2. Construir la liga congelada (al menos 8 perfiles/oponentes) y medir checkpoints 0M/10M/50M/100M.
3. Agregar manifiesto/restore exacto y selector de checkpoint campeón.

**Sprint de aprendizaje:**
4. Rango de 1.326 combos y codificación completa de apuestas, con tests de visibilidad.
5. Modelo actor–critic v2; comparar con MLP 64→32→6 bajo mismo presupuesto.
6. Ingestar HandHQ y agregar oponentes externos/solver según la variante.

**Sprint de escalado:** paralelización, batching y GPU cuando la matemática y la evaluación sean confiables.

**Regla de decisión:** no destinar otros 100M–1.000M de manos solo a la arquitectura actual sin evidencias de mejora robusta. Cualquier experimento tiene que registrar hipótesis, seed, commit, datos, contrincantes, costes, métricas y checkpoint promovido.

## Referencias

- [Estado técnico y reproducibilidad de 100M](PROJECT_STATUS.md)
- [Neural 6-max y entrenamiento](NEURAL_SIXMAX.md)
- [Motor jugable 6-max](SIXMAX.md)
- [Históricos PHH](PHH_INTEGRATION.md)
- [Resultados verificables 100M](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44564068762)
