# Historial de cambios — SolverPK

> Bitácora funcional orientada a ingeniería. Para el estado vigente, las métricas completas y el traspaso consultar [PROJECT_STATUS.md](PROJECT_STATUS.md); para propuestas futuras [ROADMAP.md](ROADMAP.md).

## 2026-11-19 — entrenamiento neuronal 100M verificado

- **Nuevo entrenamiento completado:** diez bloques de 10 millones de manos sintéticas 6-max cada uno, continuando la MLP previamente distilada. [Ejecución verificada](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44564068762).
- **Reproducibilidad:** ejecución del commit `0bca3c95a31149d8cb246dd89afd92cc5ffc8170`, semillas y learning rates por bloque registrados en `.github/workflows/neural-100m.yml`; 44 tests aprobados, confirmación automática `CONFIRMED_100M_HANDS=100000000`.
- **Artefactos:** checkpoints al finalizar cada 10M, hitos 10/50/100M publicados como artefactos independientes y evidencia completa; SHA-256 del checkpoint final registrado en [PROJECT_STATUS.md](PROJECT_STATUS.md).
- **Evaluación:** congelada contra profesor tabular y PHH simplificado, 120.000 manos por prueba. A 100M: +60,114 ±14,417 bb/100 y +47,346 ±8,090 bb/100, respectivamente.
- **Regresión de generalización:** el rival PHH fue más explotable por la red inicial (+112,355 bb/100) que por el checkpoint 100M (+47,346). Conclusión: aumentar manos no garantiza mejor estrategia para todas las distribuciones.

## 2026-11-19 — implementación de red neuronal real

- Nuevo `src/neural.rs`: MLP 64→32 ReLU→6 con softmax legal, retropropagación, aprendizaje REINFORCE y persistencia `SPKNN001`.
- Integrado en `src/sixmax.rs`: vector de 64 observaciones propias/públicas, partidas completas, recompensa terminal, evaluación congelada.
- Distilación opcional desde profesor tabular mediante entropía cruzada; [ejecución inicial](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44562105608).
- [Distilación 1M](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44562688853): ~9,3M actualizaciones neuronales, checkpoint utilizado para arrancar los 100M de self-play.
- **Aclaración histórica:** antes de esta incorporación, nuestros «modelos de aprendizaje» eran tablas actor-critic, **no redes neuronales**.

## 2026-11-19 — arena 6-max y pruebas de generalización

- Nuevo motor `src/sixmax.rs`, separado del Heads-Up previo. Ciegas rotativas, all-ins, reglas de acciones, side pots, showdown y seis acciones abstractas.
- Estados tabulares iniciales ampliados con textura de board, historial resumido de presión y creencias públicas de tres clases; formato `SPK6MAX3`.
- Evaluaciones congeladas contra random, clones e imitadores PHH; control de suma cero y partidas emparejadas.
- [Ablación PHH 1M](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44550178708): la inicialización a partir del imitador histórico fue peor que entrenar desde cero en un duelo directo (scratch +46,604 ±5,598 bb/100). No generalizar este resultado a aprendizaje histórico de alta fidelidad.

## 2026-11-19 — ingestión histórica PHH y evaluación

- Importador Rust con reconstrucción de estados, identificación de actor, formato `SPKOBS02` y entrenamiento del baseline `SPKIMT01`.
- [10.000 manos de Pluribus](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44541033820) aceptadas sin rechazos; **91.356 decisiones**.
- Comparaciones entre acciones históricas y checkpoints Heads-Up, manteniendo explícito el mismatch 6-max/Heads-Up. Ver [HISTORICAL_PLURIBUS_BENCHMARK.md](HISTORICAL_PLURIBUS_BENCHMARK.md).
- No se ha procesado ni entrenado todavía la red sobre 100M de historiales humanos; los 100M del experimento más reciente son **self-play simulado**.

## 2026-11-19 — bases Heads-Up anteriores

- Agentes Monte Carlo y actor-critic tabular Heads-Up, simulador determinista, correcciones de seed/blind, manejo all-in, evaluación de siete cartas y controles estadísticos.
- Pruebas congeladas mostraron que más entrenamiento (10M→20M tabular) no produce mejora monótona. La arena HU y sus checkpoints se mantienen por separado.

## Documentación actualizada

- [README.md](README.md): entrada principal y comandos vigentes.
- [PROJECT_STATUS.md](PROJECT_STATUS.md): fuente técnica detallada y handoff.
- [ROADMAP.md](ROADMAP.md): prioridades P0/P1/P2 con criterios de aceptación.
- [NEURAL_SIXMAX.md](NEURAL_SIXMAX.md), [SIXMAX.md](SIXMAX.md), [PHH_INTEGRATION.md](PHH_INTEGRATION.md): especificaciones y limitaciones alineadas al estado actual.

**Próxima decisión recomendada:** no ejecutar otra campaña masiva en la red de 2,3k parámetros sin primero auditar las reglas, construir una liga congelada multidistribución y fijar checkpoints realmente reproducibles. Esto está detallado en [ROADMAP.md](ROADMAP.md).
