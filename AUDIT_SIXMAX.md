# Auditoría de SolverPK: reglas, rake, ganancias y evaluación

**Fecha:** 19 de noviembre de 2026. **Estado:** motor 6-max corregido y verificado; modelo neuronal histórico reevaluado sin reentrenarlo.

## Dictamen

Había un error real de reparto: un bote empatado de 28 fichas podía pagarse **15/13 en lugar de 14/14**. También había problemas de reapertura de apuestas, tamaños anunciados, validación de historiales/checkpoints e intervalos de confianza. El motor no cobraba rake: los resultados anteriores eran exclusivamente de una simulación sin comisión.

La liquidación actual separa aportes, devoluciones, pagos y comisión, y verifica en cada mano:

```text
sum(pagos) + rake = sum(aportes)
neto[jugador] = pago[jugador] - aporte[jugador]
sum(netos) = -rake
```

La reevaluación **no elimina los winrates elevados contra los dos bots internos**. Con el motor corregido y un escenario de rake del 5%, cap de 3 BB y no-flop-no-drop, el checkpoint 100M obtiene **+41,943 ±7,801 bb/100 contra PHH** en la semilla histórica. Dos semillas nuevas producen **+38,767 ±7,854** y **+38,829 ±7,796**. Es evidencia de ventaja contra ese imitador concreto; no acredita rendimiento frente a jugadores competitivos.

El modelo sigue siendo un prototipo con limitaciones estratégicas importantes: su observación no conserva el palo concreto de las cartas propias, recorta la recompensa de aprendizaje y enfrenta referencias muy comprimidas. No es un solver GTO.

## Alcance y trazabilidad

- Base inspeccionada: `ac49c32060f6c78ae9ba4514a4b82161c31e8525`.
- Motor y benchmark verificados: `0b7356c7460e8a4ff80dd78483aceeadb175bdab`.
- [Ejecución completa de la auditoría y reevaluación](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44587114212).
- Evidencia numérica permanente: [audit/2026-11-19/evaluation.json](audit/2026-11-19/evaluation.json).
- Resumen de validación y contadores: [audit/2026-11-19/validation.json](audit/2026-11-19/validation.json).
- Código central: [accounting.rs](src/accounting.rs), [sixmax.rs](src/sixmax.rs), [phh.rs](src/phh.rs), [neural.rs](src/neural.rs), [imitation.rs](src/imitation.rs).

Se revisaron el motor multijugador, ranking de cartas, flujo de apuestas, recompensas, métricas, carga de checkpoints, replay PHH y automatizaciones. Las utilidades Heads-Up antiguas también se revisaron estáticamente; conservan una abstracción distinta y **no reciben el rake del motor 6-max**. Las limitaciones HU pendientes se enumeran al final.

## Correcciones y pruebas que las justifican

| Área | Defecto o carencia | Comportamiento corregido |
|---|---|---|
| Reparto de empates | Cada aporte distinto, incluso de un jugador retirado, generaba otra división y otro resto | Se fusionan capas contiguas con los mismos jugadores elegibles; los side pots genuinos permanecen separados |
| Rake | No existía comisión ni configuración económica | Porcentaje en puntos básicos, cap global por mano, condición de flop y elección explícita en CLI |
| Apuestas no igualadas | El exceso se devolvía como una capa de payout, sin una distinción económica útil para rake | Una capa con un único aportante se devuelve completa y queda fuera de la base de comisión |
| Reapertura 6-max | Un booleano bloqueaba una resubida aun después de acumular incrementos suficientes | Se compara lo que enfrenta cada jugador desde su última acción con la última subida completa |
| Tamaños de raise | El tamaño “pot” omitía el importe que primero había que igualar | Las fracciones usan `pot + call`, respetando mínimo y stack |
| Runout sin apuestas posibles | Podía producir decisiones artificiales cuando quedaba una sola pila con fichas y nadie debía pagar | La siguiente calle termina sus apuestas sin generar esos checks artificiales |
| Duelo con comisión | El segundo resultado se obtenía negando el primero | Se acumulan ambos equipos por separado; dos clones pueden perder ambos por rake |
| Intervalos 6-max | Varianza poblacional y un único grupo podían dar incertidumbre cero | Varianza muestral por reparto agrupado; mínimo de dos grupos independientes |
| Replay PHH | Admitía manos truncadas, saltos de calls pendientes y apuestas a side pots vacíos; rechazaba algunos runouts válidos | Board completo, respuesta al all-in antes de avanzar, runout sin checks artificiales y derechos individuales coherentes en máscara/validación |
| Checkpoints | Baselines NaN, bytes sobrantes, pesos finitos desmesurados y sumas de contadores podían aceptarse o desbordar | Validación de formato/rangos, sumas comprobadas y rechazo de datos inválidos |
| Actualizaciones neuronales | El contador podía desbordar; imitación con LR cero incrementaba `steps` | Contador saturado y congelación completa cuando no hay actualización |
| Accuracy PHH | Todas las acciones empatadas en probabilidad contaban como correctas | Un único argmax legal y desempate determinista; probabilidades y log-loss no cambian |
| Automatización | Cambiar código podía lanzar entrenamientos históricos costosos | Experimentos antiguos solo mediante ejecución manual; validación de software automática |

### Reproducción del error monetario

Seis stacks de 200, botón en 0, SB en 1 y BB en 2. Secuencia preflop:

```text
asiento 3: raise a 5
asiento 4: raise a 10
asiento 5: call 10
asientos 0, 1, 2 y 3: fold
asientos 4 y 5: check hasta showdown
board: escalera real que ambos juegan
```

Aportes finales: `[0,1,2,5,10,10]`. Hay un único bote de 28 y dos ganadores. La versión original construía capas de 5, 4, 9 y 10, y entregaba dos restos al mismo asiento: 15/13. **La suma cero pasaba**, por lo que verificar únicamente conservación no detectaba el reparto incorrecto.

La prueba de regresión falló con esos valores antes de corregir el código. El pago actual es 14/14. En la contabilidad fraccionaria también se comprueban empates entre tres jugadores, donde todavía pueden existir unidades indivisibles.

### Alcance de las reglas de all-in

La reapertura es individual: después de subir a 20, dos all-ins cortos a 30 y 40 pueden reabrir al primer jugador si la última subida completa era 18. Un jugador que ya pagó 30 solo enfrenta 10, por lo que no recupera el mismo derecho.

Este caso requiere stacks desiguales; la configuración habitual reinicia los seis stacks en 100 BB. Por eso no debe atribuirse automáticamente a este defecto el winrate del entrenamiento histórico. El error del bote 28 sí se reproduce con los stacks iguales originales.

## Contrato económico implementado

Las apuestas conservan la abstracción de 200 fichas por stack y BB de 2 fichas. La liquidación usa **10.000 unidades monetarias por ficha**, es decir, **20.000 por BB**. La unidad mínima de pago es **0,00005 BB**. Una división puede pagar fracciones de la ficha usada para discretizar apuestas.

Esta separación evita que una comisión de un bote pequeño se redondee artificialmente a cero por trabajar solo con medias ciegas. Todas las operaciones de reparto y comisión usan enteros; los `f32`/`f64` se emplean en aprendizaje, normalización y presentación.

1. Se separa el exceso no igualado. Un único **aportante** genera devolución; un único **ganador elegible** con aportes de jugadores retirados sigue teniendo un bote sujeto a rake.
2. Se forman los botes según elegibilidad, sin fronteras ficticias creadas por folds.
3. Se calcula una sola comisión sobre el dinero disputado y se aplica un solo cap por mano.
4. La comisión se asigna proporcionalmente entre main y side pots. Los restos de esa asignación usan el método de mayores restos, con prioridad al main pot en empate.
5. Cada bote neto se reparte entre sus ganadores. Las unidades sobrantes se asignan en orden desde la izquierda del botón.

Un fold preflop no paga rake por defecto. Un all-in preflop que llega a mostrar el board sí ha visto flop y puede pagar. `--rake-preflop` cambia explícitamente esa política.

**No hay tarifa universal implícita:** el programa exige `--rake-bps`. `500` significa 5%; `0`, sin comisión. Un porcentaje positivo exige `--rake-cap-bb`; los caps aceptados están entre 0 y 100 BB, excluido cero, y deben poder expresarse exactamente en la unidad contable. El ejemplo 5%/3 BB es un escenario reproducible, no una réplica exacta de PokerStars u otra sala. El redondeo del porcentaje es hacia abajo a la unidad contable, documentado y probado.

```bash
# Comparación sin comisión, elegida explícitamente
cargo run --release -- --sixmax-neural-eval --hands 120000 --seed 2026111901 \
  --checkpoint neural-100m.bin --tabular scratch.bin --rake-bps 0

# Mismo modelo, escenario con comisión y no-flop-no-drop
cargo run --release -- --sixmax-neural-eval --hands 120000 --seed 2026111901 \
  --checkpoint neural-100m.bin --tabular scratch.bin \
  --rake-bps 500 --rake-cap-bb 3
```

`bb100` ya es neto después de comisión. `table_rake_bb100` informa la comisión **de toda la mesa**, no la del candidato: no debe restarse otra vez del resultado del candidato ni asumirse que paga exactamente una sexta parte.

## Validación ejecutada

La [ejecución 44587114212](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44587114212) compiló el release y pasó **95 pruebas**: 89 unitarias, 4 de integración CLI y 2 pruebas amplias ejecutadas explícitamente en release.

| Comprobación | Cobertura y resultado |
|---|---|
| Casos de regresión | Fallaron antes de cada arreglo; luego pasaron, incluyendo 15/13 frente a 14/14 |
| Evaluador independiente | 29 fixtures literales y 30.000 comparaciones de manos de 5, 6 y 7 cartas |
| Enumeración exhaustiva | **2.598.960 manos de cinco cartas**, rankings y frecuencias correctos; 7.462 rankings distintos |
| Invariancia del evaluador | Orden de cartas y las 24 permutaciones de palos |
| Contabilidad diferencial | **100.000 manos completas** contra otra implementación de liquidación |
| Tamaños de stack del estrés | 50.000 manos con stacks iguales y 50.000 con stacks desiguales |
| Acciones revisadas | **874.267**, con comprobación de dinero después de cada acción |
| Cobertura de botes | 41.957 manos con side pots; 6.072 con algún empate |
| Devoluciones | 43.705 manos; añadir un exceso no igualado no cambia rake ni netos |
| Clones y rake por CLI | Sin rake, duelo simétrico cero; con rake, ambos clones pierden su comisión |
| Congelación de modelos | SHA-256 idénticos antes y después de las 12 evaluaciones |

El estrés produjo 94.354 showdowns y 5.646 victorias por fold. Su semilla es `0xd1ce6a1120261119`. La referencia de liquidación usa umbrales de aportes de jugadores vivos y sumas acotadas; no llama al algoritmo de reparto de producción. Se probaron rake cero, 5%, 100%, caps pequeños y la condición preflop.

Las pruebas amplias aparecen como `ignored` en el comando unitario habitual para no penalizar cada iteración; **el workflow las ejecuta por separado**:

```bash
cargo test --all-targets --no-fail-fast
cargo test --release exhaustive_five_card -- --ignored --nocapture
cargo test --release audit_stress -- --ignored --nocapture
```

La revisión independiente final añadió cuatro casos PHH: runout válido con stacks desiguales, prohibición de avanzar el board con un call pendiente, rechazo de apuestas a side pots vacíos y coherencia entre máscara y ejecución. Los cuatro fallaron antes de corregirse en [44586214682](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44586214682) y pasaron en la ejecución final 44587114212. La revisión independiente aceptó las correcciones sin bloqueos pendientes.

Evidencia de regresiones antes de arreglarlas: [primer RED](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44582539425), [rake y estadística](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44583328393), [replay/checkpoints](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44584191602). Son ejecuciones deliberadas para demostrar que las pruebas detectaban los defectos; no representan el estado final.

Esto valida una cobertura concreta y amplia. No constituye una certificación completa de todas las variantes NLHE ni un cálculo de explotabilidad del agente.

## Reevaluación del checkpoint 100M

No se actualizaron los pesos ni se repitieron los 100 millones de manos. Se ejecutaron **12 casos de 120.000 manos: 1.440.000 manos de evaluación**, tres semillas, dos oponentes y dos configuraciones de rake. Cada caso tiene **20.000 grupos independientes de reparto**, con seis posiciones del botón por grupo. Las bandas son semianchos de intervalos aproximados del 95%.

### Comparación con lo publicado: misma semilla 944104

| Rival congelado | Motor histórico, rake 0 | Motor corregido, rake 0 | Corregido, 5% y cap 3 BB |
|---|---:|---:|---:|
| Profesor tabular | +60,114 ±14,417 | +56,560 ±14,292 | **+47,663 ±13,986** |
| Imitador PHH | +47,346 ±8,090 | +47,838 ±8,037 | **+41,943 ±7,801** |

Los valores anteriores permanecen como evidencia histórica del motor anterior. No son estimaciones válidas del motor corregido ni resultados después de rake.

### Semillas nuevas, fijadas antes de ejecutar

| Semilla | Rival | Corregido, rake 0 | Corregido, 5% y cap 3 BB |
|---|---|---:|---:|
| 2026111901 | Profesor tabular | +71,782 ±15,087 | +62,635 ±14,768 |
| 2026111902 | Profesor tabular | +45,857 ±14,892 | +37,139 ±14,588 |
| 2026111901 | Imitador PHH | +44,650 ±8,076 | +38,767 ±7,854 |
| 2026111902 | Imitador PHH | +44,776 ±8,031 | +38,829 ±7,796 |

La mejora o caída por cambiar el motor incluye cambios de trayectoria debidos al nuevo tamaño de apuestas. Dentro de cada par corregido con/sin rake, las políticas congeladas enfrentan los mismos repartos y acciones; el cambio económico se liquida al final.

**Conclusión de los datos:** el rake reduce el resultado, pero no explicaba por sí solo el +47 histórico. El imitador PHH agrupa **960 estados y 3 acciones**, no observa el board completo ni los tamaños/historial, y aprende de todos los actores del dataset, no exclusivamente de Pluribus. El profesor tabular también es una referencia limitada. Ganar mucho contra ellos no demuestra un winrate extrapolable a una mesa real ni que los 100M produjeran una política robusta.

El script [scripts/audit_evaluate.py](scripts/audit_evaluate.py) verifica hashes, registra los parámetros, comprueba la comisión máxima y que evaluar no modifique los modelos. Las semillas no se seleccionaron a partir de sus resultados.

SHA-256 del checkpoint neuronal conservado:

```text
a712525d815872286eb960f330b5f585e4218fef627f7276b2e2e1adb08a8ac4
```

## Mejoras siguientes, por prioridad

1. **Observación neuronal versionada.** `Ah7h` y `Ac7c` pueden producir la misma entrada sobre un board de corazones: el encoder conoce si las cartas propias son del mismo palo, pero no cuál. El evaluador de showdown sí distingue correctamente las manos. Hace falta codificar los palos propios y su relación con el board, con fixtures de color/proyecto, formato de checkpoint nuevo y entrenamiento nuevo.
2. **Objetivo de aprendizaje coherente con EV.** La recompensa terminal se recorta y la ventaja vuelve a recortarse. Por ejemplo, 82% de resultados +20 BB y 18% de −100 BB da EV −1,6 BB, pero recompensa media +0,10 tras el primer recorte. Diseñar una actualización que conserve el objetivo de dinero esperado y controlar la varianza de otra manera.
3. **Gradientes por trayectoria coherentes.** La red recoge una mano y actualiza cada decisión secuencialmente, recalculando probabilidades con pesos ya modificados. Acumular gradientes sobre una instantánea o adoptar un algoritmo que corrija explícitamente esa diferencia.
4. **Liga de evaluación.** Diversificar rivales, perfiles y semillas; registrar cobertura PHH y tamaños de acciones, comparar varios checkpoints y varias inicializaciones. Mantener aparte entrenamiento, selección y test final.
5. **Checkpoints reproducibles y durables.** Guardado temporal y reemplazo atómico; registrar manos, semillas/posición del generador, configuración de rake, versión del motor/observaciones y hashes. Los artefactos históricos de Actions tienen retención limitada.
6. **Consolidar o retirar las arenas HU antiguas.** Aún usan abstracciones de apuestas distintas, sin rake, y parte de sus intervalos trata como independientes manos que comparten reparto por parejas. Las cifras nuevas de este informe provienen exclusivamente del motor 6-max corregido.

Reentrenar el mismo encoder actual durante más millones de manos no puede recuperar la información sobre palos que nunca recibe. La prioridad estratégica es esa corrección y una evaluación más exigente, antes de invertir en otro entrenamiento masivo o GPU.

## Referencias de reglas y modelado

- [Poker TDA 2026, reglas 21, 23, 45 y 49](https://www.pokertda.com/view-poker-tda-rules/): restos, side pots, aumentos mínimos y reapertura por incrementos acumulados. Se usa como referencia de reglas, no como certificación de una sala cash.
- [PokerStars: explicación de pot, side pots y call incluido](https://www.pokerstars.com/help/articles/poker-rules-master/229151/).
- [PokerStars: rake publicado](https://www.pokerstars.com/poker/room/rake/?no_redirect=1): ejemplo de no-flop-no-drop y de por qué tarifas/redondeos dependen del operador.
- [Maryland Lottery and Gaming Control Agency, Poker Standard Rules v1.6, capítulo 17.D](https://www.mdgaming.com/wp-content/uploads/2026/02/Poker-Standard-Rules-Version-1.6.pdf): exclusión de apuestas finales no igualadas de la base de rake.
