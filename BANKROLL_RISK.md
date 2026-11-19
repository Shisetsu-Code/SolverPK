# SolverPK: caja USD 50, límite de apuesta USD 10 y stop-loss

**Estado:** implementado y verificado en [GitHub Actions #44605811794](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44605811794), conclusión success. La integración está en `src/sixmax_bankroll.rs`, expuesta mediante `--sixmax-bankroll-eval`. Se evaluaron checkpoints tabulares 10M y 20M, sin modificarlos.

**Corrección posterior:** la [comparación con motores externos](EXTERNAL_ENGINE_AUDIT.md) encontró que un stop inferior a la caja podía sobrepasarse durante la última mano y que el contador de all-in omitía calls y raises recortados al stack. El código actual reserva la entrada completa antes de repartir, cuenta todos los all-in efectivos e incluye las ciegas en la exposición. La tabla de sesiones de este documento conserva los resultados históricos del run citado; no se reetiquetan sus contadores como si fueran de la nueva versión.

## Contrato de riesgo y unidades

- **Caja inicial: USD 50** (`--bankroll-usd 50`).
- **Exposición monetaria máxima: USD 10 por jugador y mano** (`--max-bet-usd 10`). La mesa 6-max fija el stack máximo de cada jugador en **100 BB**, con BB monetaria de **USD 0,10**. Por tanto 100 BB equivalen a USD 10. Esto es un máximo total por mano: no se permite apostar USD 10 adicionales en cada calle; el total comprometido por la misma mano nunca supera USD 10.
- **Stop-loss: USD 50 desde la caja inicial** (`--stop-loss-usd 50`), configurable en dólares enteros entre 1 USD y la caja inicial. Es un límite duro, referido a la caja inicial. Para autorizar otra mano se exige `saldo - USD 10 >= caja inicial - stop`. Si quedan fondos pero no presupuesto de pérdida para esa entrada, se detiene con `stop_loss_budget`. Con entrada fija de USD 10 y stop inferior a USD 10 no comienza ninguna mano. No es trailing stop de pérdidas desde el pico. También se detiene si el saldo cae por debajo de USD 10.
- La caja se actualiza exclusivamente con `settlement.net[0]`, ya neto de rake: beneficio neto simulado del asiento candidato y pérdidas acumuladas. En el experimento citado se utilizó rake del 5 %, tope 3 BB y no-flop-no-drop.
- Cada partida se simula con la misma pila fija de 100 BB; el **saldo entre manos** se lleva en una cuenta separada. Antes de cada mano se exige poder financiar los USD 10 íntegros. No se utiliza el saldo creciente para subir la apuesta por encima de USD 10.
- Se informa balance final, beneficio/pérdida total, USD ganados por cada 100 manos realmente jugadas, BB por cada 100 manos, máximo drawdown desde el pico, cantidad de all-in **solo del candidato**, mayor apuesta incremental real y exposición máxima real en una mano.
- Esta función es **evaluación**, no un cambio de la estrategia histórica ni un stop-loss automático dentro de los modos previos `--sixmax-eval` y `--sixmax-train`. Se usan exclusivamente checkpoints congelados.

## Reproducción

```bash
cargo run --release -- --sixmax-bankroll-eval \
  --checkpoint tabular-corrected-10m.bin \
  --opponent phh --phh-model pluribus-imitation.bin \
  --hands 100000 --seed 990071 \
  --bankroll-usd 50 --max-bet-usd 10 --stop-loss-usd 50 \
  --rake-bps 500 --rake-cap-bb 3
```

Un `--max-bet-usd` distinto de 10 se **rechaza** en esta arena fija, en lugar de simular un tope ficticio. Para stakes distintos hay que parametrizar primero BB, buy-in y apuestas, manteniendo un contrato coherente.

## Evidencia histórica de 30 sesiones con cinco semillas comunes

Cinco seeds (`990071`, `990073`, `990079`, `990081`, `990083`) × tres rivales × dos checkpoints. Se reutilizan las mismas cinco semillas entre modelos y rivales: no son 30 muestras completamente independientes. Cada sesión solicita hasta 100.000 manos, pero se interrumpe si la caja o el stop-loss impiden continuar. La tabla es **descriptiva** y no representa probabilidades de ruina confiables de población.

| Checkpoint | Rival | Sesiones finalizadas / 5 | Sesiones sin fondos para USD 10 / 5 | Detalle |
|---|---|---:|---:|---|
| 10M | PHH simplificado | 5 | 0 | Ganancia USD **12,61–14,36 por cada 100 manos**; las cinco completaron 100k |
| 20M | PHH simplificado | 5 | 0 | Ganancia USD **9,49–11,05 por cada 100 manos**; las cinco completaron 100k |
| 10M | Aleatorio legal | 3 | **2** | Dos sesiones terminaron tras solo **67 y 43 manos** |
| 20M | Aleatorio legal | 3 | **2** | Dos sesiones terminaron tras **576 y 43 manos** |
| 10M | Clones propios | 0 | **5** | Entre **125 y 8.606** manos antes de no financiar otra entrada |
| 20M | Clones propios | 0 | **5** | Entre **341 y 7.757** manos antes de no financiar otra entrada |

Todas las ejecuciones confirmaron **USD 10 como máximo de exposición total en una mano** y **USD 10 como máximo de apuesta individual**. La ganancia máxima de una mano de seis jugadores puede superar USD 10 porque el límite es el **riesgo propio**, no el premio del bote.

En el run histórico, por 100k manos contra PHH, el tabular 10M seleccionó la categoría de acción all-in entre **5.511 y 5.722** veces; el 20M entre **4.362 y 4.431** veces. Esas cifras **no cuentan todos los all-in efectivos**: excluyen calls que agotan el stack y otras categorías de raise recortadas al stack. El código posterior cuenta la transición real a stack cero y ya no usa la categoría seleccionada como sustituto.

**Conclusión crítica:** el nuevo límite de USD 10 **no convierte** las ganancias extraordinarias contra PHH en algo realista; demuestra que están presentes pese a tener la exposición limitada por mano. Y bajo caja USD 50, se observa riesgo de ruina alto frente a otros rivales. El límite de USD 10 equivale al **20% de la caja inicial por mano**, una exposición extremadamente alta. La estrategia puede perder varias entradas consecutivas sin necesidad de emplear una martingala.

Las semillas muestran que los resultados de sesiones truncadas son muy distintos del promedio sobre 100.000 manos siempre completadas. No hay forma honesta de inferir rentabilidad real ni una tasa poblacional de quiebra usando cinco sesiones por rival.

## Tests de seguridad

La ejecución histórica pasó **127 unit tests + 17 tests de integración = 144 pruebas automáticas**. Sus seis pruebas de riesgo no cubrían el sobrepaso de un stop menor ni los all-in omitidos. La auditoría externa agrega regresiones observadas primero en fallo y después en éxito para esos casos, el motivo de cierre en la última mano y parámetros inválidos. Véase [EXTERNAL_ENGINE_AUDIT.md](EXTERNAL_ENGINE_AUDIT.md) para la evidencia actual. Los checkpoints se conservan.

**Siguiente prioridad técnica:** ampliar el número de seeds con datos completos de distribución de supervivencia/ruina; evaluar un **límite de pérdida diario inferior a USD 50**, y controles de tamaño de apuesta según porcentaje del capital (p. ej. 1–2 % de la caja, que con USD 50 sería USD 0,50–1,00 de exposición, no USD 10). Parametrizar stakes/stack correctamente antes de permitir esos límites, sin reescalar apuestas artificialmente.
