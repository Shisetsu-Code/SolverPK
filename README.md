# SolverPK

Simuladores y agentes de investigación para **Texas Hold'em** en Rust, sin dependencias externas en el núcleo actual. Incluye Heads-Up tabular/Monte Carlo, arena no-limit 6-max, ingestión PHH y una **política neuronal V2**. También conserva el modelo histórico V1 entrenado con **100 millones de manos sintéticas adicionales de self-play**.

**Estado actual (2026-11-19):** V2 conserva la identidad de las cartas visibles, usa recompensa monetaria lineal y actualiza la política una vez por mano. La [validación de V2](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44594831850) pasó **140 pruebas**: 121 unitarias, 17 de CLI y dos auditorías amplias en release. El piloto de tres semillas, con 100.000 manos de entrenamiento cada una, **no produjo un campeón**: mejoró contra el rival uniforme y empeoró en promedio contra los otros siete rivales. No se lanzó otro entrenamiento de 100M. [NEURAL_V2.md](NEURAL_V2.md) reúne cambios, protocolo, resultados y evidencia; [AUDIT_SIXMAX.md](AUDIT_SIXMAX.md) conserva la auditoría económica anterior.

**Documentación principal:**

La [auditoría externa posterior](EXTERNAL_ENGINE_AUDIT.md) verifica el motor 6-max con PokerKit/Treys: 6.000 manos completas, 38.806 casos de cartas, 158 pruebas Rust y nueve métodos Python. Incluye las correcciones más recientes de stop loss, telemetría y subidas con stacks desiguales.

- [EXTERNAL_ENGINE_AUDIT.md](EXTERNAL_ENGINE_AUDIT.md): comparación reproducible con PokerKit y Treys; correcciones de riesgo, telemetría y subidas con stacks desiguales.
- [BANKROLL_RISK.md](BANKROLL_RISK.md): caja USD 50, máximo USD 10 por mano y stop loss; resultados históricos de sesiones y límites del experimento.
- [AUDIT_SIXMAX.md](AUDIT_SIXMAX.md): revisión actual de reglas, rake, ganancias, pruebas independientes y reevaluación del checkpoint 100M.
- [NEURAL_V2.md](NEURAL_V2.md): correcciones V2, validación y piloto con controles iniciales y tres semillas de entrenamiento.
- [PROJECT_STATUS.md](PROJECT_STATUS.md): inventario técnico, cronología, evidencia, parámetros, checkpoints, comandos, limitaciones y traspaso a otro desarrollador/LLM.
- [CHANGELOG.md](CHANGELOG.md): historial cronológico de cambios y experimentos validados.
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

Todos los comandos 6-max exigen `--rake-bps`: `500` equivale a 5%; `0` elige explícitamente comisión cero. Con rake positivo también se exige `--rake-cap-bb`. Los ejemplos usan 5% con cap de 3 BB y no-flop-no-drop; es un escenario configurable, no la tarifa de una sala específica.

La red actual es `128 → 32 ReLU → 6 softmax`, con **4.326 pesos y sesgos**. Observa 52 indicadores de cartas propias, 52 del board visible y 24 valores públicos; conserva rango y palo, y no depende del orden de las cartas. Sus seis acciones son fold, check/call y cuatro tamaños abstractos de subida. En self-play, cada decisión usa el retorno neto de su propio jugador, dividido por 100 BB, sin recortar recompensa ni ventaja; todos los gradientes de una mano se calculan antes de actualizar pesos o baseline.

El formato nuevo es **`SPKNN002`**. Guarda versiones de arquitectura/observación/optimizador, contadores, semillas e intervalos de entrenamiento, learning rate, rake y procedencia; valida una suma de integridad y usa guardado atómico. `SPKNN001` conserva exactamente su inferencia histórica de 64 entradas y es **solo evaluable**. Ninguno de los dos formatos es intercambiable con los checkpoints tabulares.

```bash
# Crear V2 en un archivo nuevo
cargo run --release -- --sixmax-neural-train --rake-bps 500 --rake-cap-bb 3 --hands 100000 --seed 73001 --learning-rate 0.0005 --checkpoint neural-v2.bin

# Reanudar V2: la misma semilla continúa desde el siguiente índice de mano
cargo run --release -- --sixmax-neural-train --rake-bps 500 --rake-cap-bb 3 --resume --hands 100000 --seed 73001 --learning-rate 0.0005 --checkpoint neural-v2.bin

# Liga congelada: cinco referencias heurísticas incluidas por defecto
cargo run --release -- --sixmax-neural-league --rake-bps 500 --rake-cap-bb 3 --hands 12000 --seed 990071 --checkpoint neural-v2.bin

# Añadir varios checkpoints congelados; --rival-checkpoint se puede repetir
cargo run --release -- --sixmax-neural-league --rake-bps 500 --rake-cap-bb 3 --hands 12000 --seed 990073 --checkpoint neural-v2.bin \
  --rival-checkpoint audit/2026-11-19/neural-v2/models/neural-v1-100m.bin \
  --rival-checkpoint audit/2026-11-19/neural-v2/models/neural-v2-73001-initial.bin

# Profesor tabular opcional, para una primera distilación supervisada
cargo run --release -- --sixmax-train --rake-bps 500 --rake-cap-bb 3 --hands 100000 --seed 42 --checkpoint tabular.bin
cargo run --release -- --sixmax-neural-distill --rake-bps 500 --rake-cap-bb 3 --teacher tabular.bin --hands 100000 --seed 43 --checkpoint distilled-v2.bin

# Evaluación: candidato neural vs 5 profesores tabulares
cargo run --release -- --sixmax-neural-eval --rake-bps 500 --rake-cap-bb 3 --hands 120000 --seed 990071 --checkpoint distilled-v2.bin --tabular tabular.bin
```

Una ejecución nueva rechaza un archivo de destino existente. `--resume` acepta únicamente V2 y mantiene el índice de reparto de cada semilla; no convierte ni reentrena V1. El flujo manual actual es [neural-v2-train.yml](.github/workflows/neural-v2-train.yml).

La liga incluye uniforme legal, calling station, tight-aggressive, loose-aggressive y nit. Son referencias heurísticas sin calibración competitiva. `--hands` cuenta manos **por rival**; los intervalos se calculan sobre grupos de seis rotaciones del botón. Se rechaza el solapamiento con repartos de entrenamiento registrados, pero la procedencia incompleta de V1 y de profesores externos se informa explícitamente. La liga no selecciona automáticamente un campeón.

**Modelos conservados:** los seis controles/checkpoints V2 del piloto están en [audit/2026-11-19/neural-v2/models](audit/2026-11-19/neural-v2/models). El [checkpoint V1 de 100M](audit/2026-11-19/neural-v2/models/neural-v1-100m.bin) mantiene su SHA-256 `a712525d815872286eb960f330b5f585e4218fef627f7276b2e2e1adb08a8ac4`. En la comparación de compatibilidad con el motor corregido y rake del 5%, cap de 3 BB, reprodujo **+47,663 bb/100 contra el tabular** y **+41,943 contra PHH**; ese resultado corresponde a V1. Ver protocolo y evidencia en [NEURAL_V2.md](NEURAL_V2.md).

## Experimento histórico V1 de 100M, anterior a la auditoría

**Las cifras de esta sección corresponden a V1 y al motor anterior, sin rake.** Se conservan para trazabilidad; los resultados corregidos están en [AUDIT_SIXMAX.md](AUDIT_SIXMAX.md). La [receta original](docs/history/workflows/neural-100m-v1.yml) está archivada y requiere el código del commit `0bca3c95a31149d8cb246dd89afd92cc5ffc8170` y sus archivos originales.

| Punto de evaluación | Contra profesor tabular | Contra imitador PHH |
|---|---:|---:|
| Inicial, tras 1M manos de distilación | −22,33 bb/100 | +112,36 bb/100 |
| 10M nuevas | +24,15 bb/100 | +29,53 bb/100 |
| 50M nuevas | +36,95 bb/100 | +44,43 bb/100 |
| **100M nuevas** | **+60,11 bb/100** | **+47,35 bb/100** |

Cada par de cifras proviene de **120.000 manos** con la misma semilla de test y rivales congelados. Mejoró claramente frente al tabular, pero **empeoró respecto al checkpoint inicial frente a PHH**. Por eso, el checkpoint más reciente no es automáticamente el más robusto. Los intervalos y la metodología están documentados en [PROJECT_STATUS.md](PROJECT_STATUS.md).

## Motor 6-max tabular y replay histórico

```bash
cargo run --release -- --sixmax-train --rake-bps 500 --rake-cap-bb 3 --hands 1000000 --seed 42 --checkpoint sixmax.bin
cargo run --release -- --sixmax-eval --rake-bps 500 --rake-cap-bb 3 --hands 120000 --seed 99 --checkpoint sixmax.bin --opponent random

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

El motor conserva cuatro tamaños abstractos de raise. Su liquidación pasó 100.000 manos contra otra implementación y el evaluador pasó las 2.598.960 manos de cinco cartas; esto no certifica todas las variantes NLHE. V2 sigue siendo una política pequeña, sin historial completo, critic neuronal ni rangos de 1.326 combinaciones. La liga amplía la evaluación, pero sus rivales no demuestran calidad competitiva ni permiten medir explotabilidad.

**Próxima prioridad:** investigar la degradación observada en el piloto, comparar modificaciones con controles y varias semillas, y exigir robustez frente a los distintos rivales antes de ampliar el entrenamiento o elegir un campeón. Ver [NEURAL_V2.md](NEURAL_V2.md) y [ROADMAP.md](ROADMAP.md).

Uso previsto: simulación y evaluación de estrategias fuera de línea. Licencia MIT.
