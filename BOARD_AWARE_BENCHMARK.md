# Evaluación congelada con rivales que usan las cartas comunitarias

**Fecha:** 20 de noviembre de 2026. **Candidatos:** tabular 10M y tabular 20M, política del asiento 0 de cada checkpoint. **Unidad principal:** ganancia neta por cada 100 manos realmente jugadas, en BB y USD.

## Resultado del experimento completo

La [ejecución completa](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44708043940) terminó correctamente sobre el commit `85d7fd05f46d1841a1bd2fb41351785c4ad4ef8f`. Se conservaron las **1.280 sesiones de las 20 combinaciones**, con **661.289 manos realmente jugadas** de un máximo programado de 2.560.000. Hubo 1.119 cierres por falta de fondos para la siguiente entrada y 161 sesiones que alcanzaron las 2.000 manos. No se reponen sesiones detenidas ni se seleccionan las favorables.

**Los dos tabulares pierden en las 18 combinaciones con rivales sensibles al board. Solo ganan contra el control PHH histórico.** Los intervalos aproximados por condición de esas 18 tasas quedan por debajo de cero. No se corrigieron las tasas imponiéndoles un techo: se mantuvieron las reglas y se cambiaron los adversarios de evaluación.

### Condición principal: 100 BB, rake 5%, cap 3 BB

| Rival | Tabular | Manos jugadas | BB/100 netas | USD/100 netos | IC 95% BB/100 | Cierres / 64 | Neto medio por sesión |
|---|---:|---:|---:|---:|---:|---:|---:|
| Equity multiway | 10M | 5.708 | −498,66 | −49,87 | [−615,09; −409,85] | 64 | USD −44,47 |
| Equity multiway | 20M | 6.223 | −449,40 | −44,94 | [−555,03; −370,77] | 64 | USD −43,70 |
| TAG | 10M | 15.986 | −175,79 | −17,58 | [−211,76; −147,97] | 64 | USD −43,91 |
| TAG | 20M | 19.331 | −145,03 | −14,50 | [−178,93; −119,78] | 64 | USD −43,81 |
| LAG | 10M | 27.151 | −85,19 | −8,52 | [−137,10; −48,63] | 61 | USD −36,14 |
| LAG | 20M | 28.936 | −72,95 | −7,29 | [−124,96; −36,86] | 59 | USD −32,98 |
| PHH histórico | 10M | 124.417 | +135,17 | +13,52 | [122,18; 147,84] | 2 | USD +262,78 |
| PHH histórico | 20M | 126.311 | +106,53 | +10,65 | [96,96; 116,12] | 1 | USD +210,25 |

Cada tasa usa las manos que ese candidato pudo jugar. Frente a equity, las 64 sesiones del 10M duraron en promedio 89,19 manos; las del 20M, 97,23. Por eso una pérdida de aproximadamente USD 45 por sesión puede expresarse como cerca de USD 45–50 por cada 100 manos. No implica una apuesta individual de ese importe: la máxima exposición observada por mano fue USD 10.

El saldo inicial de USD 50 no se pierde necesariamente hasta cero: con stack de USD 10 la sesión deja de iniciar manos cuando el saldo cae por debajo de USD 10. Esto conserva el límite de pérdida desde la caja inicial. El drawdown desde un máximo previo puede superar USD 50 si primero hubo ganancias; el stop configurado no es un trailing stop.

### Sensibilidad: 50 BB, rake 5%, cap 3 BB

| Rival | Tabular | Manos jugadas | BB/100 netas | USD/100 netos | IC 95% BB/100 | Cierres / 64 | Neto medio por sesión |
|---|---:|---:|---:|---:|---:|---:|---:|
| Equity multiway | 10M | 11.910 | −250,88 | −25,09 | [−286,87; −220,12] | 64 | USD −46,69 |
| Equity multiway | 20M | 13.315 | −226,25 | −22,63 | [−258,67; −198,76] | 64 | USD −47,07 |
| TAG | 10M | 34.426 | −87,05 | −8,70 | [−101,41; −75,05] | 64 | USD −46,82 |
| TAG | 20M | 43.218 | −67,61 | −6,76 | [−80,71; −57,32] | 62 | USD −45,66 |
| LAG | 10M | 41.506 | −62,97 | −6,30 | [−85,27; −46,15] | 59 | USD −40,84 |
| LAG | 20M | 49.792 | −47,51 | −4,75 | [−66,40; −32,62] | 56 | USD −36,96 |

La entrada y exposición observada se reducen a USD 5. Que la pérdida por 100 manos disminuya no demuestra una mejora de estrategia: también cambió la profundidad y el dinero arriesgado por mano. Los dos modelos siguen perdiendo frente a los tres rivales.

### Sensibilidad: 100 BB sin rake

| Rival | Tabular | Manos jugadas | BB/100 netas | USD/100 netos | IC 95% BB/100 | Cierres / 64 | Neto medio por sesión |
|---|---:|---:|---:|---:|---:|---:|---:|
| Equity multiway | 10M | 6.175 | −465,10 | −46,51 | [−574,51; −384,23] | 64 | USD −44,88 |
| Equity multiway | 20M | 6.528 | −435,99 | −43,60 | [−539,15; −358,91] | 64 | USD −44,47 |
| TAG | 10M | 16.791 | −168,36 | −16,84 | [−203,80; −141,20] | 64 | USD −44,17 |
| TAG | 20M | 20.361 | −139,07 | −13,91 | [−171,58; −114,82] | 64 | USD −44,24 |
| LAG | 10M | 32.341 | −59,93 | −5,99 | [−108,40; −26,60] | 58 | USD −30,29 |
| LAG | 20M | 30.863 | −57,35 | −5,74 | [−109,06; −21,70] | 57 | USD −27,66 |

Las pérdidas persisten sin rake. Por tanto, la comisión no explica por sí sola el fracaso contra estos rivales. El efecto causal del rake sobre una política con horizonte ilimitado no se estima aquí: al cambiar el rake también cambia la duración de las sesiones detenidas.

### Qué aporta pasar de 10M a 20M

En la condición principal, la diferencia 20M − 10M fue **+49,26 BB/100** frente a equity (IC pareado 95%: +9,26 a +96,92), **+30,76** frente a TAG (+4,02 a +61,62), **+12,24** frente a LAG (−32,94 a +53,49) y **−28,64** frente a PHH (−35,39 a −21,96). Son comparaciones descriptivas sin ajuste por multiplicidad, entre dos checkpoints concretos; no son réplicas independientes de entrenamiento.

El 20M pierde más despacio frente a equity, pero **ambos cierran las 64 sesiones**. La diferencia en ganancia media por sesión es solo USD +0,78, con intervalo −0,09 a +1,64. La mejora de BB/100 no basta para afirmar una mejora financiera de la sesión ni para promover un modelo rentable. No se seleccionó campeón.

### Interpretación técnica y próximos cambios

El adaptador tabular conserva la misma política del asiento 0, clave de estado y softmax legal del CLI histórico. En `state(View)`, el tabular representa las cartas propias mediante cinco categorías gruesas y el board mediante textura; **no representa qué mano concreta forman juntas las cartas propias y comunitarias ni su equity**. El PHH llama a `predict_multi` sin pasar el board. Estas limitaciones permiten que un modelo explote al PHH y, a la vez, pierda contra rivales que reaccionan a la fuerza de la mano. El experimento demuestra la debilidad frente a estos adversarios; no aísla causalmente cuánto aporta cada defecto de representación.

Además, el entrenamiento tabular aún recorta retorno y ventaja: las ganancias superiores a +100 BB se saturan en la recompensa. Ese recorte pertenece al aprendizaje; **los resultados monetarios de esta evaluación no están recortados**. La red neuronal V2 ya tiene representación exacta de cartas y recompensa lineal, pero no fue evaluada en esta corrida.

Orden propuesto antes de otra campaña masiva:

1. Evaluar V2 y un control nuevo con este mismo protocolo, manteniendo un conjunto final de semillas distinto para selección y confirmación. Registrar políticas/checkpoints por asiento para no presentar el asiento 0 como si fuera todo el self-play.
2. Corregir o reemplazar la representación tabular: mano formada, proyectos, blockers y equity frente a rangos públicos; usar recompensa monetaria lineal y comparar cada cambio por separado.
3. Entrenar pilotos contra una población variada, con varias inicializaciones y controles congelados. Medir pérdida por calle, posición, tamaño de bote y decisiones de all-in; exigir mejora en adversarios reservados y en ganancia por sesión.
4. Añadir rivales competitivos externos y rangos condicionados por acciones. Los motores PokerKit/Treys sirven como referencias de reglas/evaluación de cartas; no certifican fuerza estratégica ni rentabilidad humana.

## Evidencia conservada

- [results-summary.json](audit/2026-11-20/board-aware/results-summary.json): las 20 tasas, intervalos, cierres, saldos, drawdowns y las diez diferencias pareadas.
- [results.json.gz](audit/2026-11-20/board-aware/results.json.gz): resultado íntegro, 1.280 registros de sesión y comandos exactos.
- [execution-logs.tar.gz](audit/2026-11-20/board-aware/execution-logs.tar.gz): salidas originales de cada proceso y experimento corto completo.
- [manifest.json](audit/2026-11-20/board-aware/manifest.json), [verification.json](audit/2026-11-20/board-aware/verification.json), [validation.json](audit/2026-11-20/board-aware/validation.json): identidad del código, binario/modelos/protocolo, comprobación de inmutabilidad, CI y revisión.
- [SHA256SUMS](audit/2026-11-20/board-aware/SHA256SUMS): integridad de la evidencia y de los modelos comprimidos.

## Qué cambió

La auditoría anterior verificó reglas y dinero, pero el rival histórico PHH no recibe el board en su predicción. La liga ya contenía TAG/LAG que leen las cartas visibles mediante reglas sencillas. Esta ampliación añade `equity_multiway`, un rival independiente de los pesos del candidato que estima su participación en el bote con simulaciones de cartas desconocidas.

El nuevo modo `--sixmax-risk-benchmark` evalúa candidatos tabulares o neuronales mediante el runner congelado compartido. Conserva el mazo y las reglas auditadas, separa el RNG de decisiones del RNG del reparto e incorpora los controles de caja a ambas familias de políticas. El experimento principal utiliza los dos tabulares; no modifica ni entrena sus pesos.

Los checkpoints originales se conservan comprimidos sin pérdida en [audit/2026-11-20/board-aware/models](audit/2026-11-20/board-aware/models), con SHA-256 de sus bytes originales en el [protocolo](audit/2026-11-20/board-aware/protocol.json). La reproducción no depende de la fecha de vencimiento de los artefactos históricos de Actions.

## Protocolo fijado antes de ejecutar

| Parámetro | Valor |
|---|---|
| Caja inicial por sesión | USD 50 |
| Stop loss desde la caja inicial | USD 50 |
| Máximo permitido por mano | USD 10, incluidas las ciegas |
| Valor de la BB | USD 0,10 |
| Sesiones por combinación | 64 |
| Horizonte por sesión | Hasta 2.000 manos; no se repone una sesión detenida |
| Condición principal | 100 BB de stack, rake 5%, cap 3 BB, no-flop-no-drop |
| Sensibilidades | 50 BB con el mismo rake; 100 BB sin rake |
| Rivales principales | Equity multiway, tight-aggressive y loose-aggressive |
| Control histórico | PHH, únicamente en la condición principal |
| Comparaciones | 20 combinaciones; máximo programado 2.560.000 manos |
| Muestras del rival de equity | 96 por decisión |
| Semilla principal | `2026112007` |
| Remuestreo estadístico | 5.000 muestras de sesiones completas, seed `2026112011` |

Un stack de 50 BB supone una entrada real de USD 5. El máximo permitido sigue siendo USD 10, pero antes de repartir se reserva la entrada efectiva. Las escalas de observación neuronal mantienen la referencia de 100 BB con la que se entrenaron los modelos. La caja persiste entre manos; todos los participantes vuelven al stack declarado al comenzar cada mano y los rivales se recargan.

Cada sesión ocupa un bloque distinto de índices: `sesión * horizonte + mano`. La semilla de reparto se obtiene aplicando una única vez la secuencia XOR/stride a ese índice global; anidar la misma operación por sesión y por mano duplicaría repartos. El botón inicial es `sesión % 6` y rota en cada mano. Los modelos y condiciones comparten bloques deliberadamente para comparar resultados emparejados.

Se verifican los 128.000 índices previstos frente a intervalos conocidos de entrenamiento y evaluación, incluyendo los dos seeds que inicializan el mismo estado del RNG. La corrida corta usa otra semilla (`2026112099`) y 24 índices, excluidos del experimento completo y viceversa. Para tabulares, la procedencia se acredita con el manifiesto externo vinculado al hash; no se afirma conocer exposición histórica no registrada.

## Cómo decide el rival de equity

Recibe solamente `View`: cartas propias, board visible y estado público. Excluye las cartas conocidas y muestrea sin reemplazo las cartas comunitarias restantes y dos cartas para cada rival activo. Compara contra todos ellos y, en un empate, cuenta `1 / número de ganadores` del bote. Su RNG depende de los conjuntos de cartas propias y públicas y del número de activos; nunca recibe la semilla del mazo real ni cartas ocultas.

La regla de apuestas es fija: compara equity con `call / (pot + call)` y utiliza un umbral de valor `max(0,45; 1/activos + 0,18)`. Usa probabilidades declaradas de fold/call/raise y los cuatro tamaños legales del motor. El código exacto está en [src/sixmax_equity.rs](src/sixmax_equity.rs).

Es un rival diagnóstico con rangos uniformes. No condiciona las manos rivales a sus apuestas, no estima realización futura de equity ni EV específico de botes secundarios y no es un solver GTO. La mera utilización del board no acredita fuerza humana. TAG/LAG siguen siendo heurísticas de categoría absoluta; PHH sigue siendo el imitador comprimido histórico.

## Cómo se calculan los resultados

Se retiene cada sesión, incluidas las detenidas y las que no pueden comenzar. Los registros conservan enteros para dinero, manos, exposición y rake. El neto ya incluye rake; la comisión de toda la mesa se informa por separado y no se vuelve a descontar al candidato.

```text
BB/100  = 100 × suma(profit_units) / (20.000 × suma(hands_played))
USD/100 = 100 × suma(profit_units) / (200.000 × suma(hands_played))
```

No se promedian tasas de sesiones de distinta duración. Cero manos produce una tasa no disponible, no un falso equilibrio. Se informa además ganancia media por sesión, saldo y drawdown, proporción de cierres financieros con intervalo Wilson, y tasa por horizonte programado con ese denominador explícito.

El bootstrap remuestrea sesiones completas. En el diferencial 20M − 10M conserva el emparejamiento y recalcula el denominador propio de cada candidato. Las 20 condiciones no multiplican el número de réplicas independientes: siguen siendo 64 bloques de sesión compartidos. Los intervalos son aproximados, por condición y sin ajuste por comparaciones múltiples; no se elige un campeón a partir del mejor resultado observado.

Se separan `reached_horizon` y `can_continue`: una sesión puede jugar su última mano solicitada y terminar sin dinero suficiente para otra. Las tasas observadas describen este protocolo de detención; no equivalen automáticamente al rendimiento de una política con bankroll ilimitado ni a ganancias frente a personas.

## Integración verificada

Los cambios se integraron mediante [PR #4](https://github.com/Shisetsu-Code/SolverPK/pull/4) en el commit `7126c07178577c1719a24827c0b363584e3f4e4b`, cuyo árbol coincide exactamente con la rama revisada. En `main` terminaron correctamente los workflows de [contabilidad](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44710111148), [benchmark y estadísticas](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44710111157) y [comparación con motores externos](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44710111203).

La integración verificó 181 pruebas Rust distintas (177 ordinarias y las cuatro auditorías explícitas), 21 pruebas Python estadísticas y nueve de regresión del comparador. El replay externo volvió a comprobar 6.000 manos y 72.541 estados/acciones, sin errores, con 6.000 liquidaciones netas exactas bajo las mismas convenciones de rake/reparto. Se mantienen las diferencias de reglas y redondeo ya documentadas en [EXTERNAL_ENGINE_AUDIT.md](EXTERNAL_ENGINE_AUDIT.md). También pasaron los 38.806 casos PokerKit/Treys, la enumeración de cinco cartas y el estrés contable. El [registro de integración](audit/2026-11-20/board-aware/integration.json) conserva los identificadores, resultados y alcance. El cierre documental posterior no modifica código, protocolo ni modelos.

## Verificación y reproducción

La [prueba inicial](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44706186593) reprodujo siete fallos del estimador todavía sin implementar y ocho fallos de la nueva interfaz ausente. La [verificación posterior](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44707518790) aprobó 177 pruebas Rust ordinarias, 21 de Python, el estrés contable de 100.000 manos y la enumeración de 2.598.960 manos de cinco cartas. Completó también los 20 casos cortos, 480 manos en total, con modelos sin cambios.

Las pruebas cubren privacidad, empates multiway, cartas simuladas sin duplicados, sensibilidad real al board, máscaras legales, entradas de USD 5 y USD 10, sesiones sin manos, cierres coincidentes con el horizonte, exclusiones de semillas y conservación monetaria. Una revisión independiente examinó tanto el código Rust como el protocolo y los cálculos estadísticos.

El workflow [board-aware-benchmark.yml](.github/workflows/board-aware-benchmark.yml) compila y verifica todo, restaura los modelos del repositorio y ejecuta el smoke. La ejecución completa se activa manualmente con `full=true` o en la rama de evaluación con el marcador `[run-benchmark]`, después de las mismas verificaciones. Los pushes a `main` repiten validación y smoke.

```bash
cargo test --all-targets --no-fail-fast
python scripts/board_benchmark_tests.py -v
cargo build --release
python scripts/board_benchmark.py --binary target/release/solverpk \
  --ten references/ten/tabular-corrected-10m.bin \
  --twenty references/twenty/tabular-corrected-20m.bin \
  --phh references/phh/pluribus-imitation.bin --output results
```

El directorio de salida debe ser nuevo. Los modelos se descomprimen según el paso correspondiente del workflow. El runner guarda el manifiesto antes de ejecutar, los comandos, todos los registros de sesión, resultados parciales si hay errores, hashes y la verificación final. No acepta un experimento con casos faltantes ni modelos, binario o protocolo modificados durante la ejecución.
