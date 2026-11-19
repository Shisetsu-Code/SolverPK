# SolverPK

Simuladores y agentes de investigación para **Texas Hold'em** en Rust, sin dependencias externas en el núcleo actual. Incluye Heads-Up tabular/Monte Carlo, arena no-limit 6-max, ingestión PHH y **una red neuronal real entrenada con 100 millones de manos sintéticas de self-play**.

**Estado confirmado (2026-11-19):** entrenamiento 6-max neuronal de 100M completado en [GitHub Actions](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44564068762), con los diez bloques y 44 pruebas exitosos. El agente final obtuvo +60,11 ±14,42 bb/100 contra su profesor tabular y +47,35 ±8,09 bb/100 contra un imitador PHH. Son resultados **del simulador abstracto**, no GTO ni rendimiento esperado ante humanos.

**Documentación principal:**

- [PROJECT_STATUS.md](PROJECT_STATUS.md): inventario técnico, cronología, evidencia, parámetros, checkpoints, comandos, limitaciones y traspaso a otro desarrollador/LLM.
- [ROADMAP.md](ROADMAP.md): mejoras propuestas en orden de prioridad, con criterios de aceptación.
- [NEURAL_SIXMAX.md](NEURAL_SIXMAX.md): arquitectura neuronal, aprendizaje y resultados.
- [SIXMAX.md](SIXMAX.md): motor multijugador, reglas y self-play tabular.
- [PHH_INTEGRATION.md](PHH_INTEGRATION.md): formato, reconstrucción e ingesta de historiales PHH.
- [HISTORICAL_PLURIBUS_BENCHMARK.md](HISTORICAL_PLURIBUS_BENCHMARK.md): diagnósticos basados en decisiones históricas.

## Requisitos

Toolchain estable Rust (edition 2021). No se necesitan Python, PyTorch ni crates de ML externos para compilar este proyecto. El entrenamiento actual es **CPU**; no se ha integrado ROCm/Vulkan ni GPU.

```bash
cargo test --all-targets
cargo build --release
cargo run --release -- --help
```

## Entrenar y evaluar: red neuronal 6-max

Red `64 → 32 ReLU → 6 softmax`, backpropagation con política REINFORCE y baseline móvil; observación parcial sin cartas ocultas, acciones fold/check-call/4 tamaños de subida. Checkpoint neuronal `SPKNN001`, **distinto de los formatos tabulares**.

```bash
# Entrenar política neuronal desde cero con manos sintéticas 6-max
cargo run --release -- --sixmax-neural-train --hands 20000 --seed 42 --checkpoint neural.bin

# Retomar una política ya entrenada
cargo run --release -- --sixmax-neural-train --resume --hands 100000 --seed 43 --learning-rate 0.0005 --checkpoint neural.bin

# Profesor tabular opcional, para una primera distilación supervisada
cargo run --release -- --sixmax-train --hands 100000 --seed 42 --checkpoint tabular.bin
cargo run --release -- --sixmax-neural-distill --teacher tabular.bin --hands 100000 --seed 43 --checkpoint distilled.bin

# Evaluación: candidato neural vs 5 profesores tabulares
cargo run --release -- --sixmax-neural-eval --hands 120000 --seed 944104 --checkpoint neural.bin --tabular tabular.bin
```

**Checkpoint real de 100M:** descargar desde [run 44564068762](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44564068762), artifact `neural-100m-checkpoint-100m` o `neural-100m-full-evidence`. El archivo de pesos no está versionado dentro de Git; su SHA-256 está registrado en [PROJECT_STATUS.md](PROJECT_STATUS.md). Guardar una copia fuera de GitHub Actions, que actualmente usa retención de 30 días.

## Qué cambió con los 100M

| Punto de evaluación | Contra profesor tabular | Contra imitador PHH |
|---|---:|---:|
| Inicial, tras 1M manos de distilación | −22,33 bb/100 | +112,36 bb/100 |
| 10M nuevas | +24,15 bb/100 | +29,53 bb/100 |
| 50M nuevas | +36,95 bb/100 | +44,43 bb/100 |
| **100M nuevas** | **+60,11 bb/100** | **+47,35 bb/100** |

Cada par de cifras proviene de **120.000 manos** con la misma semilla de test y rivales congelados. Mejoró claramente frente al tabular, pero **empeoró respecto al checkpoint inicial frente a PHH**. Por eso, el checkpoint más reciente no es automáticamente el más robusto. Los intervalos y la metodología están documentados en [PROJECT_STATUS.md](PROJECT_STATUS.md).

## Motor 6-max tabular y replay histórico

```bash
cargo run --release -- --sixmax-train --hands 1000000 --seed 42 --checkpoint sixmax.bin
cargo run --release -- --sixmax-eval --hands 120000 --seed 99 --checkpoint sixmax.bin --opponent random

# Importar PHH (p. ej. data/pluribus del repo público)
cargo run --release -- --import-phh --source data/pluribus --output observations.bin
cargo run --release -- --train-phh --source observations.bin --output imitation.bin
```

Se importaron y validaron las **10.000 manos de Pluribus** (91.356 decisiones); el modelo de imitación es una aproximación tabular de acciones, **no el algoritmo original de Pluribus**. Todavía no se entrenó el modelo neuronal con 21,6M de HandHQ ni las colecciones completas ACPC.

## Heads-Up original (compatibilidad)

```bash
cargo run --release -- --hands 10000 --seed 42 --samples 64
cargo run --release -- --self-play --hands 10000 --rooms 64 --agents 8 --samples 8 --checkpoint selfplay.bin
cargo run --release -- --self-play --resume --hands 10000 --rooms 64 --agents 8 --samples 8 --checkpoint selfplay.bin
```

El Heads-Up usa política **tabular de 640 buckets y 3 acciones**, con Monte Carlo/evaluador de siete cartas. Las 64 «rooms» son **lógicas y secuenciales**, no 64 hilos. Sus checkpoints **no** pueden cargarse en el motor neuronal ni en el 6-max tabular.

## Límites y orden de próximas mejoras

El motor es una abstracción con 4 tamaños de raise y sin rake; no ha superado una suite independiente de conformidad de apuestas, all-ins y side pots. La política neuronal de ~2.278 parámetros no modela el historial completo ni rangos de 1.326 combos; REINFORCE usa recompensa terminal de alta varianza. Solo se han contrastado unos pocos rivales fijos; no hay garantía de equilibrio, explotabilidad o GTO.

**Próxima prioridad:** 1) auditar las reglas con una implementación independiente, 2) construir liga congelada y selección de mejor checkpoint con semillas de test nuevas, 3) checkpoints reproducibles/retomables, 4) rangos completos e historial de apuestas, 5) actor–critic neuronal mejorado, 6) paralelización y GPU AMD cuando se justifiquen. Los criterios detallados están en [ROADMAP.md](ROADMAP.md).

Uso previsto: simulación y evaluación de estrategias fuera de línea. Licencia MIT.
