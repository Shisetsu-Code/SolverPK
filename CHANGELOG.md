# Historial de cambios — SolverPK

## 2026-11-20 — evaluación congelada con board y caja finita

- Nuevo rival `equity_multiway`: estima participación en el bote frente a todas las manos activas, con cartas públicas/propias, muestreo sin reemplazo y empates fraccionados. Usa rangos uniformes; no es GTO.
- Nuevo `--sixmax-risk-benchmark` para candidatos tabulares/neuronales con runner congelado compartido, caja/stop USD 50 y máximo USD 10 por mano. Reserva la entrada efectiva: USD 5 a 50 BB o USD 10 a 100 BB.
- Índices globales de reparto por sesión, control de solapamiento con streams conocidos y seed separado para la corrida corta. Se mantienen todos los cierres financieros y sesiones sin manos.
- Runner Python con hashes antes/después, manifiesto previo, 20 combinaciones prefijadas, tasas sobre manos reales y bootstrap pareado por sesiones completas. Checkpoints originales y evidencia completa conservados en Git.
- Regresiones observadas antes de implementar; después pasaron 177 pruebas Rust ordinarias, 21 Python, estrés de 100.000 manos y enumeración de 2.598.960 manos de cinco cartas. Revisiones independientes de código, protocolo y los 1.280 registros reales.
- Experimento completo: 661.289 manos, 1.119 sesiones detenidas y 161 que alcanzaron el horizonte. Ambos modelos pierden en las 18 combinaciones con rivales sensibles al board; ganan solo frente a PHH. A 100 BB con rake 5%, el 20M obtiene −449,40 BB/100 (−USD 44,94/100) frente a equity y +106,53 BB/100 (+USD 10,65/100) frente a PHH.
- Se documenta la representación tabular limitada y el recorte de recompensa de entrenamiento, sin atribuirles una contribución causal cuantificada. No se entrenaron pesos, no se recortaron las tasas reportadas y no se seleccionó campeón. [Informe completo](BOARD_AWARE_BENCHMARK.md).

## 2026-11-19 — comparación con motores externos y controles de caja

- Comparación reproducible del evaluador con PokerKit/Treys fijados por commit y hashes: 38.806 casos, incluidas las 7.462 fuerzas distintas de cinco cartas.
- Exportación de 6.000 manos reales del motor 6-max y reproducción en PokerKit, con stacks fijos/desiguales y cobertura de las seis posiciones en cada grupo. Se registran diferencias de reglas y de convenciones monetarias.
- Coincidencia de 72.541 estados/acciones y 6.000 liquidaciones brutas y netas bajo el mismo rake y reparto entero; tolerancia monetaria cero. El comparador rechaza pagos desviados, máscaras incompletas y aportes finales incoherentes.
- 158 pruebas Rust y nueve métodos de regresión Python aprobados, incluida la enumeración de 2.598.960 manos de cinco cartas y estrés contable de 100.000 manos.
- Stop loss duro financiando previamente la entrada completa; rechazo de parámetros inválidos/duplicados y motivo financiero de cierre correcto en la última mano.
- Telemetría de all-in por agotamiento real del stack, incluyendo calls y raises recortados; exposición de ciegas aunque el candidato no tome decisiones.
- Se impiden subidas adicionales cuando todos los rivales están cubiertos por la apuesta vigente.
- Los hallazgos se reprodujeron con regresiones antes de corregirlos. Informe, alcance y evidencia: [EXTERNAL_ENGINE_AUDIT.md](EXTERNAL_ENGINE_AUDIT.md). No se reentrenaron ni sustituyeron checkpoints en esta auditoría.

## 2026-11-19 — Neural V2 y evaluación ampliada

- Encoder 128D: cartas propias y board visible exactos, incluidos rango/palo; entrada invariante al orden de cartas, sin filtración de cartas ocultas.
- Retorno monetario lineal después de rake y una actualización por mano con política/baseline congelados. Acumulación f64 y gradientes contrastados por diferencias finitas.
- SPKNN002 con esquema, segmentos de entrenamiento, checksum, reanudación exacta y publicación atómica. SPKNN001 conserva inferencia y pasa a ser exclusivamente evaluable.
- CLI estricta y protección contra sobrescritura accidental de checkpoints; liga de cinco perfiles más rivales neuronales/tabular/PHH opcionales, con controles de solapamiento de repartos.
- 140 pruebas aprobadas, incluido el estrés económico anterior. Piloto de tres inicializaciones, 100k manos cada una: mejora frente a uniforme y deterioro de medias ante otros siete rivales; no se selecciona campeón.
- Receta original 100M archivada en `docs/history/workflows/neural-100m-v1.yml`; workflow nuevo manual para V2. Reporte y evidencia: [NEURAL_V2.md](NEURAL_V2.md).


## 2026-11-19 — auditoría económica posterior a los 100M

- Corregidos reparto de empates/side pots, tamaños calculados después del call, reapertura acumulativa de all-ins cortos y runouts sin apuestas posibles.
- Nuevo `accounting.rs`: rake explícito, cap único, no-flop-no-drop, devoluciones excluidas, enteros de precisión fina y cierre de dinero por mano.
- Duelo y evaluación reportan resultados netos; ambos equipos se acumulan por separado y los intervalos usan varianza muestral por reparto agrupado.
- Corregidos derechos de subida y validación de historiales PHH, accuracy en empates, carga de checkpoints y desbordamientos de contadores.
- Añadidos oráculo independiente de cartas, enumeración de 2.598.960 manos, 100.000 partidas con referencia independiente de liquidación e integración CLI.
- Reevaluación congelada: 12 casos, 1,44M manos, tres semillas, dos rivales y dos perfiles de rake; hashes de pesos intactos. [Evidencia](AUDIT_SIXMAX.md).
- Entrenamientos históricos pasan a ejecución manual; la validación del software se ejecuta automáticamente.
- Hallazgos estratégicos pendientes: identidad de palos perdida en las observaciones, recompensa recortada, actualizaciones secuenciales con política modificada y referencias de evaluación muy comprimidas.

> Bitácora funcional orientada a ingeniería. Para el estado vigente, las métricas completas y el traspaso consultar [PROJECT_STATUS.md](PROJECT_STATUS.md); para propuestas futuras [ROADMAP.md](ROADMAP.md).

## 2026-11-19 — entrenamiento neuronal 100M verificado

- **Nuevo entrenamiento completado:** diez bloques de 10 millones de manos sintéticas 6-max cada uno, continuando la MLP previamente distilada. [Ejecución verificada](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44564068762).
- **Reproducibilidad:** ejecución del commit `0bca3c95a31149d8cb246dd89afd92cc5ffc8170`, semillas y learning rates por bloque registrados en la [receta histórica](docs/history/workflows/neural-100m-v1.yml); 44 tests aprobados, confirmación automática `CONFIRMED_100M_HANDS=100000000`.
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

**Próxima decisión recomendada:** después de las correcciones V2, mejorar generalización y validación de la política antes de otra campaña masiva; la liga y los checkpoints reproducibles básicos ya están implementados. Esto está detallado en [ROADMAP.md](ROADMAP.md).
