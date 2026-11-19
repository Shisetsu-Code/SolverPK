# Neural V2: correcciones, validación y resultados

**Fecha:** 19 de noviembre de 2026. **Estado:** integrado en main mediante [PR #2](https://github.com/Shisetsu-Code/SolverPK/pull/2); [CI de main](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44598247851): 140 pruebas aprobadas. [CI completa](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44595765332), commit `c94321087fb114edfd562be448fb401c0d210a12`.

## Dictamen

Se corrigieron las tres limitaciones concretas identificadas tras la auditoría económica: pérdida de información de palos, recompensa monetaria recortada y actualización de decisiones con políticas distintas. Se añadió evaluación contra más rivales, procedencia de entrenamiento, reanudación exacta y protección de checkpoints.

**La corrección del software no demostró una mejora general de fuerza estratégica.** El piloto de tres redes V2 nuevas mejoró su media frente al rival uniforme y empeoró frente a los otros siete. No se seleccionó campeón ni se sustituyó el checkpoint histórico 100M.

## Cambios técnicos

| Área | V1 histórico | V2 actual |
|---|---|---|
| Red | 64→32→6, 2.278 parámetros | **128→32→6, 4.326 parámetros** |
| Cartas | Rangos propios, suitedness y conteos marginales de board | Plano de 52 cartas propias y plano de 52 cartas públicas visibles; conserva asociación rango/palo |
| Contexto público | 19 cantidades comprimidas además de cartas | 24 cantidades normalizadas; posición, apuestas, stacks y resúmenes públicos |
| Retorno | Neto/20BB recortado y ventaja recortada otra vez | **Neto después de rake /100BB**, escala lineal sin esos recortes |
| Actualización | Pesos y baseline cambiaban entre visitas de la misma trayectoria | Una actualización por mano; todos los gradientes usan los mismos pesos/baseline anteriores |
| Formato | SPKNN001 | SPKNN002; dimensiones y versiones explícitas |
| Continuación | Se repetían índices de reparto al reiniciar una corrida | Segmentos por seed e índice absoluto; N+M igual a una corrida continua bajo igual binario/configuración |
| Persistencia | Escritura directa | Temporal sincronizado y publicación atómica; creación nueva sin reemplazar archivos existentes |
| Evaluación | Dos referencias internas | Cinco estilos heurísticos y múltiples checkpoints neuronales, tabular y PHH opcionales |

Los planos de cartas son invariantes al orden de las cartas, y nunca consultan cartas ajenas ni board futuro. La versión legacy reproduce exactamente sus 64 entradas originales. V1 se puede cargar/evaluar y guardar sin reinterpretar sus pesos; **no se permite entrenarlo o destilarlo como si fuera V2**.

### Objetivo monetario y gradiente

Para el jugador `p`, `R_p = net_units[p] / (BB_UNITS * 100)`. Así, −100 BB corresponde a −1 y +500 BB a +5. La apuesta de ejemplo con 82% de +20 BB y 18% de −100 BB tiene EV −1,6 BB y recompensa media **−0,016**; antes el recorte daba +0,10.

Se acumula en f64 `G = (1/6) Σ_p Σ_t (R_p − b_anterior) ∇log π(a_pt|s_pt)` y se aplica una actualización. El divisor es la cantidad de jugadores, no la cantidad variable de decisiones. Cada visita utiliza el retorno propio de su jugador, no el resultado total de la mesa. Después se actualiza el baseline una sola vez con la media de los seis retornos.

Es el gradiente simultáneo de agentes independientes con parámetros compartidos, manteniendo fijas las políticas rivales al derivar cada jugador. **No es el gradiente del dinero total de la mesa ni una garantía de equilibrio o de mejora contra cualquier rival.** La proyección de pesos a [−8,8] permanece; no se añade clipping de norma, retorno o ventaja. LR cero mantiene pesos, baseline y contadores de actualización.

Referencia matemática: [Spinning Up — policy optimization y baselines](https://spinningup.openai.com/en/latest/spinningup/rl_intro3.html). Implementación y diferencias finitas: [neural.rs](src/neural.rs) y [neural_v2_tests.rs](src/neural_v2_tests.rs).

### Procedencia, reanudación y archivos

SPKNN002 registra formato/arquitectura, parámetros, baseline, pasos, actualizaciones y segmentos: tipo de aprendizaje, seed, índice inicial, manos, LR y rake. Los segmentos equivalentes contiguos se fusionan. Una continuación con la misma seed avanza desde el siguiente índice registrado, incluyendo la rotación de botón.

El lector limita el tamaño, rechaza versiones/dimensiones incorrectas, valores no finitos, pesos fuera de rango, segmentos solapados/desbordados, bytes sobrantes y checksum incorrecto. FNV-1a64 detecta corrupción accidental; no es una firma criptográfica. Los manifiestos de esta entrega añaden SHA-256 y commit del código.

El guardado usa un temporal en el mismo directorio y `sync_all`. Para una ruta nueva se publica mediante hard link que falla si el destino ya existe; para `--resume` se reemplaza atómicamente. En Unix también se sincroniza el directorio. Se requiere un sistema de archivos con esas operaciones; un error no trunca el checkpoint previo. Un error de sincronización posterior a publicar puede dejar el archivo completo y reportar durabilidad incierta.

La destilación desde un tabular marca procedencia externa incompleta, porque su formato no registra toda la exposición del profesor. Reanudar self-play no borra esa marca. El estado de juego/RNG se reconstruye por reparto a partir de seed e índice; no se reanuda una mano a mitad de ejecución. Los cambios intencionales de LR/rake/seed se registran como nuevos segmentos.

## Validación ejecutada

**140 pruebas aprobadas, cero fallidas:** 121 unitarias, 17 integraciones CLI y dos auditorías en release. Las dos auditorías figuran como ignored en el comando normal y se ejecutan explícitamente por separado.

- Cuatro regresiones de cartas: flush propio, asociación rango/palo en mano y board, orden de hole cards.
- Derivadas numéricas independientes de los cuatro grupos de parámetros; máscara legal, ReLU, retorno completo y baseline no nulo.
- Cancelación exacta en política congelada, reversión del orden de visitas, retorno propio en juego de suma cero y normalización por jugadores.
- Continuación de self-play y destilación: pesos/metadatos idénticos entre corrida completa y corrida fragmentada.
- Checkpoints legacy: inferencia/bytes preservados y entrenamiento rechazado; rutas nuevas protegidas y argumentos inválidos rechazados.
- Liga congelada: legalidad, estilos distintos, privacidad, hash sin cambios, reproducibilidad y detección de reutilización de repartos.
- Auditorías económicas anteriores: **2.598.960 manos de cinco cartas**; **100.000 partidas completas y 874.267 acciones** contra referencia de liquidación.

[Primer RED](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44592407208): 23 regresiones reprodujeron los defectos. [Segundo RED](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44593734388): las correcciones centrales pasaron y las dos pruebas nuevas reprodujeron argumentos mal aceptados/sobrescritura. [GREEN final](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44595765332): suite y piloto completos.

Dos revisiones independientes del núcleo/persistencia y una revisión del workflow no encontraron bloqueos. Fueron revisiones estáticas; la ejecución de Rust y los experimentos se verificaron en Actions. El proyecto conserva avisos preexistentes de código/imports no usados.

## Experimento acotado

- Inicializaciones de V2: **73001, 73002, 73003**; **100.000 manos de self-play por red**, LR 0,0005; 300.000 manos con actualización en total.
- Control inicial: mismos pesos de inicialización, sin aprendizaje; una mano con LR cero solo registra procedencia, no modifica la política.
- Test: seeds **990071 y 990073**, 12.000 manos por rival y corrida, 2.000 grupos independientes de seis posiciones de botón.
- Rivales: uniforme, calling station, nit, tight-aggressive, loose-aggressive, cinco copias del neuronal V1 100M, tabular y PHH.
- Perfil económico: **5% de rake, cap 3 BB por mano, no-flop-no-drop**; los bb/100 ya son netos.
- También se evaluó el V1 100M como candidato en esa misma liga. Total: **112 filas y 1.344.000 manos de liga**; adicionalmente 240.000 manos de compatibilidad histórica.
- Sin búsqueda de hiperparámetros ni selección de modelo a partir del test. Repetir el CI reprodujo los seis SHA-256 V2; esas repeticiones **no se suman como muestras independientes**.

### Medias descriptivas, bb/100 netos

| Rival | V2 inicial | V2 tras 100k por red | V1 histórico 100M |
|---|---:|---:|---:|
| Calling station | -116,81 | -201,16 | -0,83 |
| Loose-aggressive | -1362,87 | -1865,07 | +13,93 |
| Cinco copias del V1 100M | -480,81 | -714,91 | -8,18 |
| Nit | -87,87 | -96,46 | +19,57 |
| Imitador PHH | +11,22 | -23,57 | +30,10 |
| Profesor tabular | -943,67 | -959,87 | +44,38 |
| Tight-aggressive | -594,53 | -787,03 | -15,94 |
| Uniforme legal | -97,18 | +565,91 | +7,13 |

Las columnas V2 promedian tres inicializaciones y dos semillas de test (72.000 manos por rival y fase); V1 promedia dos semillas (24.000 manos por rival). **Los presupuestos de entrenamiento son muy diferentes:** V1 acumula 100M de self-play y la destilación previa de 1M; V2 parte de cero con 100k por red. La comparación sirve para diagnosticar comportamiento, no para concluir que una arquitectura es superior bajo igual presupuesto.

Cada corrida conserva su intervalo aproximado por clusters en [results.json](audit/2026-11-19/neural-v2/results.json). La liga usa Student-t aproximado conservador; son intervalos por rival, sin ajuste por comparaciones múltiples. La tabla de medias no tiene un IC agregado ni una prueba de significación emparejada.

El resultado observado muestra especialización: V2 aprendió a explotar uniforme, mientras empeoraron sus medias frente a los demás estilos. Las pérdidas grandes contra LAG/TAG corresponden a políticas iniciales muy débiles en esta abstracción; no se deben interpretar como rendimientos de humanos. El 100M tampoco gana contra todos los estilos. Las heurísticas son referencias de diagnóstico, no rivales competitivos certificados.

### Compatibilidad exacta del modelo 100M

Con semilla 944104, 120.000 manos por rival y el mismo perfil 5%/3BB, la ruta legacy reproduce los resultados publicados de la auditoría anterior: **+47,663 ±13,986 bb/100 contra tabular** y **+41,943 ±7,801 contra PHH**. Estas comparaciones usan el protocolo anterior para comprobar compatibilidad; la liga nueva usa otro muestreo de acciones y otras semillas.

SHA-256 del V1 conservado: `a712525d815872286eb960f330b5f585e4218fef627f7276b2e2e1adb08a8ac4`.

## Uso actual

```bash
# Una ruta nueva; modelo V2 desde cero.
cargo run --release -- --sixmax-neural-train --hands 100000 --seed 73001 \
  --learning-rate 0.0005 --rake-bps 500 --rake-cap-bb 3 --checkpoint neural-v2.bin

# Misma seed: las siguientes 100k manos, sin repetir sus índices anteriores.
cargo run --release -- --sixmax-neural-train --resume --hands 100000 --seed 73001 \
  --learning-rate 0.0005 --rake-bps 500 --rake-cap-bb 3 --checkpoint neural-v2.bin

# Cinco estilos por defecto, más el V1 histórico como rival congelado.
cargo run --release -- --sixmax-neural-league --hands 120000 --seed 990071 \
  --rake-bps 500 --rake-cap-bb 3 --checkpoint neural-v2.bin \
  --rival-checkpoint audit/2026-11-19/neural-v2/models/neural-v1-100m.bin
```

`--hands` en la liga significa manos **por rival**. `--rival-checkpoint` se puede repetir; `--tabular` y `--phh-model` agregan referencias. Se rechazan repartos que aparezcan en segmentos conocidos, incluso si una seed maestra diferente produce los mismos repartos o uno de los dos alias del RNG. Con V1/profesores antiguos, la salida declara `unverified_legacy` o `unverified_external`; se comprueban igualmente las exposiciones que sí constan.

Para ejecutar en GitHub: [Neural v2 train and frozen league](.github/workflows/neural-v2-train.yml), manual, inicia desde cero o copia un SPKNN002 versionado seleccionado explícitamente. La [receta V1 original](docs/history/workflows/neural-100m-v1.yml) está archivada fuera de Actions y coincide con el blob original `c3ac82093860b7640c2db496e4b670e717fe7bad`.

## Evidencia conservada

- [Resultados completos](audit/2026-11-19/neural-v2/results.json), [validación](audit/2026-11-19/neural-v2/validation.json) y [SHA256SUMS](audit/2026-11-19/neural-v2/SHA256SUMS).
- [Modelos](audit/2026-11-19/neural-v2/models): tres iniciales, tres pilotos entrenados y el V1 100M, versionados en Git. Los nombres `trained` describen el entrenamiento; no indican que sean campeones.
- [Script de reproducción](scripts/neural_v2_experiment.py): semillas fijadas, hashes, comparación legacy y todas las filas de la liga.
- Logs/binario en la ejecución enlazada de CI; retención del artifact 90 días. Los pesos y JSON anteriores no dependen de esa retención.

La reproducción de las filas tabular/PHH requiere también sus archivos de referencia históricos (`scratch.bin` y `pluribus-imitation.bin`), cuyos SHA-256 y fuentes están registrados en el script/JSON. Esos dos archivos no se incorporan a este archivo de siete modelos; conservarlos sigue siendo necesario para repetir exactamente el benchmark externo. La liga de cinco heurísticas y los modelos neuronales versionados se ejecutan sin descargarlos.

## Siguientes mejoras y límites pendientes

1. Entrenar con una población diversa de rivales y validar por separado; el self-play homogéneo no mostró generalización suficiente en el piloto.
2. Reducir varianza con una estimación de valor por estado y evaluar estabilidad con más inicializaciones, conservando el objetivo monetario.
3. Incorporar historia pública de apuestas y rangos más completos; medir capacidad adicional con presupuestos comparables.
4. Ampliar procedencia del profesor/dataset, calendario de entrenamiento y selección de modelos; falta validar rivales competitivos y explotabilidad.
5. Completar conformidad externa del motor y consolidar los motores HU antiguos.

Detalle menor de la revisión: un archivo construido manualmente y con checksum recalculado puede declarar ciertos metadatos de rake fuera del rango aceptado por CLI. Esos metadatos son documentales; las reglas de la siguiente corrida provienen de la configuración CLI validada. Alinear esas restricciones del lector queda como endurecimiento menor, sin afectar los resultados de esta campaña.
