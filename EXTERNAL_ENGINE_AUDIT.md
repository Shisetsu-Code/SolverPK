# Comparación de SolverPK con motores externos

**Fecha:** 19 de noviembre de 2026. **Alcance:** motor no-limit 6-max, evaluador compartido y controles de caja del modo `--sixmax-bankroll-eval`.

## Dictamen y alcance de la evidencia

Esta auditoría contrasta cartas, decisiones y dinero con implementaciones ajenas al proyecto. La versión inicial auditada fue `2b178eb9d3424ecacdb859064b872d2d9893cc24`. Se encontraron y corrigieron errores de control de riesgo, telemetría y validación de argumentos, además de una condición de subida con stacks desiguales. Las comprobaciones de ganancias verifican netos después de rake; no convierten el benchmark contra bots simplificados en una estimación de rentabilidad contra jugadores humanos.

Los resultados finales, SHA de código, hashes de corpus y ejecuciones quedan en [audit/2026-11-19/external-engines/validation.json](audit/2026-11-19/external-engines/validation.json). El criterio de aceptación exige coincidencia entera exacta bajo las mismas reglas de comisión y fichas sobrantes. Las diferencias con un reparto racional sin redondear se conservan solo como diagnóstico. No se descartan manos fallidas para declarar éxito.

## Resultados

La [ejecución final 44613024568](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44613024568) aprobó estas comprobaciones sobre el commit `9d314670981b0c592e0eb482333e49c52a4d6bc4`. Los cambios posteriores de documentación no modifican los archivos ejecutables verificados.

| Comprobación | Resultado |
|---|---|
| Pruebas Rust ordinarias | 154 aprobadas: 133 unitarias y 21 de integración |
| Pruebas Rust amplias ejecutadas expresamente | 4 aprobadas; **158 pruebas Rust en total** |
| Regresiones del comparador Python | **9 métodos aprobados**, incluidas alteraciones que conservan la suma de dinero |
| Evaluación externa de cartas | **38.806/38.806** coincidencias frente a PokerKit y Treys |
| Enumeración interna de cinco cartas | **2.598.960 combinaciones** verificadas |
| Manos completas reproducidas externamente | **6.000/6.000**, con **72.541/72.541** estados y acciones coincidentes |
| Liquidaciones exactas sin rake y con rake | **6.000/6.000** en ambos casos; tolerancia de aceptación **cero** |
| Estrés contable adicional | **100.000 manos**, 874.267 acciones, 41.130 manos con botes secundarios y 5.982 con botes divididos |
| Manos incompletas o discrepancias desconocidas | **0** |

Los dos grupos del corpus externo contienen 500 manos por cada posición del botón. Se registran 11.441 diferencias de máscara por permitir PokerKit retirarse sin enfrentar una apuesta y 820 por su tratamiento de una primera apuesta corta; son conteos de opciones de acción, no de manos. Ninguna acción elegida fue rechazada por el motor externo. Las 16 diferencias brutas y 739 netas frente al reparto racional nativo son únicamente diagnósticos de redondeo: todas las liquidaciones coinciden exactamente tras aplicar las mismas convenciones enteras.

## Referencias seleccionadas

| Proyecto | Revisión fijada | Uso en la auditoría |
|---|---|---|
| [PokerKit](https://github.com/uoftcprg/pokerkit) | `abf795299eaac41183495a0956ce7136ce38ed64` | Reejecución completa de manos y segundo evaluador de cartas |
| [Treys](https://github.com/ihendley/treys) | `70fbaade2f9b63ee3ea41b54aed7be2c921add7e` | Evaluador independiente de cinco, seis y siete cartas |
| [PyPokerEngine](https://github.com/ishikota/PyPokerEngine) | `a52a048a15da276005eca4acae96fb6eeb4dc034` | Inspección de reglas y pruebas puntuales; no utilizado como referencia definitiva |
| [PokerRL](https://github.com/EricSteinberger/PokerRL) | `e02ea667061b96912e424231da071b6f20a262f7` | Inspección de reglas y reparto; no utilizado como referencia definitiva |

Se fijan revisiones y hashes de los módulos de referencia. Los scripts rechazan fuentes distintas de las examinadas. El simulador Rust conserva cero dependencias externas; Python y estos proyectos se utilizan únicamente para verificación.

La regla de subida mínima y reapertura se contrasta con las [reglas TDA 2026, apartados 45 y 49](https://www.pokertda.com/view-poker-tda-rules/). TDA es una referencia de torneo; no define un rake universal de cash. La documentación de [PokerKit](https://pokerkit.readthedocs.io/en/stable/simulation.html) permite personalizar reparto y rake. El modelo de comisión de SolverPK se verifica contra su contrato explícito, no se presenta como la tarifa de una sala concreta.

## Protocolo reproducible

### Evaluación de cartas

`scripts/external_evaluator.py` consulta directamente PokerKit y Treys para construir:

- 7.462 representantes de todas las fuerzas distintas de cinco cartas.
- 10.000 manos aleatorias de cada tamaño: cinco, seis y siete cartas.
- 1.344 casos límite: 28 situaciones, 24 permutaciones de palos y dos órdenes de cartas.

Son **38.806 casos**. Incluyen rueda, doble trío, tres pares, kickers, póker, elección de las mejores cinco cartas de un flush y empates usando el board. Los oráculos producen ordinales distintos del formato numérico de SolverPK: el test Rust exige primero una correspondencia estrictamente creciente de las 7.462 fuerzas, y después la clase exacta para cada muestra. No se copia el algoritmo evaluador del proyecto para generar la respuesta esperada.

Además se vuelve a ejecutar la auditoría interna exhaustiva de las **2.598.960 combinaciones físicas de cinco cartas**. La comparación externa de seis/siete cartas es muestreada; no enumera todas sus combinaciones.

### Manos completas

`src/sixmax_external_tests.rs` ejecuta el `Table6` real y exporta las cartas, estados antes de cada decisión, máscaras legales, apuestas elegidas y liquidaciones. No utiliza una reimplementación del motor para producir las trazas.

El corpus final contiene **6.000 manos**: 3.000 con stacks iniciales de 100 BB y 3.000 con stacks desiguales. Cada grupo cubre las seis posiciones del botón. `scripts/external_engine_compare.py` reproduce exactamente esas cartas y acciones en PokerKit y compara actor, calle, saldo, apuesta de la calle y jugadores retirados. También consulta las acciones que PokerKit permitiría para los seis tamaños/categorías del agente.

PokerKit recibe cantidades `Fraction`, con rake nativo desactivado. Así su reparto nativo conserva fracciones exactas y no impone su regla particular para fichas indivisibles. El adaptador muestra explícitamente las cartas conocidas y captura los botes y sus jugadores elegibles **antes** de que PokerKit elimine las manos perdedoras. Usa los rangos de PokerKit para determinar los ganadores y aplica el contrato monetario declarado a esos botes externos: precisión de enteros, rake global y fichas sobrantes en orden desde el botón. No modifica el código ni el estado interno del motor de referencia.

La aceptación no permite tolerancias monetarias: pagos, netos y devoluciones deben coincidir exactamente. También se acumulan los aportes desde las acciones efectivamente ejecutadas en PokerKit y se comparan con el registro final exportado. La comisión sigue siendo 5 %, cap único por mano de 3 BB, sin rake antes del flop y sin incluir dinero no igualado. En victorias por fold, todo el rake corresponde al único ganador. Se verifica:

```text
neto[jugador] = pago[jugador] - aporte[jugador]
suma(pagos) + rake = suma(aportes)
suma(netos) = -rake
```

La ejecución no puede aprobar con manos incompletas, acciones rechazadas, diferencias sin explicación o reaparición de la subida con rivales totalmente cubiertos. Las diferencias de política de fold gratuito y los casos concretos de reapertura insuficiente de PokerKit se registran explícitamente.

Una auditoría complementaria de **100.000 manos legales** contrasta el reparto entero exacto, botes secundarios, devoluciones y varias configuraciones de rake con el oráculo contable independiente ya existente.

### Validación del propio comparador

La revisión independiente encontró que una tolerancia global demasiado amplia podía aceptar una unidad contable asignada al perdedor, y que una máscara de acciones truncada podía saltarse parte de la comparación. Se sustituyó esa aceptación por el adaptador entero exacto, validación de esquema y contraste de aportes finales contra operaciones nativas. Las regresiones alteran deliberadamente pagos, prioridades de fichas sobrantes, reparto de rake, máscaras y aportes; deben fallar aunque la suma total se conserve. Los historiales correctos, incluidos empates con varios botes, deben seguir pasando.

El workflow exige además marcadores de ejecución de los cuatro tests Rust ignorados por defecto. Un filtro que no seleccione ningún test no puede convertir el informe en aprobado.

## Errores propios reproducidos y corregidos

### 1. Stop loss que podía excederse

Con caja USD 50, stop USD 20 y saldo USD 31, antes se autorizaba otra mano de USD 10. Si se perdía, la pérdida acumulada llegaba a USD 29. Ahora se exige poder perder la entrada completa sin cruzar el piso:

```text
saldo - entrada >= caja inicial - stop loss
```

En ese ejemplo se detiene con `stop_loss_budget`. Con el valor predeterminado caja USD 50/stop USD 50, el control de fondos ya impedía saldo negativo; el fallo afectaba especialmente a stops menores. Se mantiene el máximo USD 10 por mano y la escala de BB USD 0,10. Un stop menor que USD 10 no permite financiar una mano en esta arena fija.

### 2. All-ins y exposición mal medidos

El contador solo registraba la elección de la categoría all-in. Omitía calls que agotan el stack y raises de otro tamaño recortados al máximo disponible. Ahora cuenta el consumo real del stack. También se incluyen las ciegas aunque el candidato gane sin llegar a actuar. Las cifras históricas de selección de la categoría all-in se conservan como históricas; no se reinterpretan como conteo completo.

### 3. Subidas sin adversario capaz de disputar el incremento

En situaciones de stacks desiguales, bastaba con que otro jugador conservara fichas para autorizar una subida. Ese jugador podía estar ya cubierto por la apuesta vigente. Ahora debe existir un rival cuyo stack más apuesta de calle exceda la apuesta mayor actual. El dinero sobrante antes se devolvía, por lo que no era creación de ganancias; la acción sí podía distorsionar el bote temporal y las observaciones de la política.

El fallo se reprodujo con UTG all-in de 20 fichas, un jugador profundo y todos los demás con diez fichas o menos. El jugador profundo puede pagar o retirarse, pero no hay dinero rival para un bote lateral adicional. Este hallazgo corresponde a stacks desiguales; no demuestra un sesgo en los entrenamientos previos con 100 BB iguales para todos.

### 4. Motivo de cierre y parámetros inválidos

Una pérdida en la última mano solicitada se etiquetaba como `hands_limit`, incluso con saldo cero. Ahora se informa primero el estado financiero final. Además, `--hands` y `--seed` malformados dejan de sustituirse silenciosamente por valores predeterminados. Los flags monetarios también rechazan duplicados, incluido un flag duplicado al final sin valor. Los importes monetarios siguen aceptando dólares enteros; no se añadió soporte decimal.

Las nueve regresiones iniciales fallaron con el código anterior en el [run 44608539310](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44608539310) y pasaron tras corregirlo en el [run 44609051377](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44609051377). Una décima regresión de flags monetarios duplicados se reprodujo después en el [run 44609539981](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44609539981). La evidencia final registra su verificación posterior.

## Diferencias de las referencias que no deben copiarse

PokerKit calcula su rake nativo por bote; usar simplemente su `cap` no implementa nuestro cap global. También entrega todo un resto indivisible al primer ganador, mientras SolverPK asigna una unidad adicional por ganador, desde el botón. El [reglamento oficial de Montana](https://www.dojmt.gov/wp-content/uploads/Official-Montana-Poker-Rule-Book-revised-May-2014-Finished-Draft.pdf) respalda el máximo de una ficha impar por ganador. Con tres ganadores y resto de dos unidades, esas reglas producen repartos distintos.

Se comprobó además que PokerKit puede excluir toda la apuesta de calle del ganador por fold de su base nativa de rake, no solo la parte no igualada. Por eso la auditoría configura y calcula expresamente nuestro modelo de comisión. Los límites y redondeos dependen de la sala: por ejemplo, [PokerStars documenta redondeo mitad al par](https://www.pokerstars.com/poker/room/rake/?no_redirect=1), mientras SolverPK redondea hacia abajo una vez a su unidad contable de 0,00005 BB.

La inspección de PyPokerEngine y PokerRL encontró tratamientos de all-in cortos y fichas sobrantes incompatibles con usar esos proyectos como una autoridad automática. Se consultaron como referencias adicionales; la comparación masiva utiliza PokerKit más comprobaciones explícitas del contrato y de reglas.

## Qué significa para los resultados de entrenamiento

La auditoría comprueba las reglas y el dinero del motor 6-max. No mide la fuerza competitiva de los adversarios. El rival PHH de `src/imitation.rs` es un imitador pequeño que ni siquiera recibe las cartas comunitarias en `predict_multi`; no es Pluribus. El tabular también usa una representación de cartas muy reducida. Es razonable atribuir parte de los rendimientos extremos a ese entorno débil, pero esta auditoría no cuantifica esa contribución.

El saldo de sesión del candidato persiste, pero las manos empiezan a profundidad fija y los rivales se recargan. La caja limitada no transforma ese experimento en una mesa de dinero real con seis bankrolls persistentes. Las ganancias seguirán informándose como **ganancia neta cada 100 manos**, en BB y, cuando corresponda, USD bajo las ciegas declaradas.

La siguiente mejora estratégica es evaluar políticas congeladas contra rivales independientes que utilicen cartas comunitarias, varias semillas nuevas y configuraciones de stack/rake definidas. Añadir millones de manos contra el mismo rival simplificado no corrige por sí solo la representatividad del benchmark.

## Reproducción

El workflow [external-engine-audit.yml](.github/workflows/external-engine-audit.yml) descarga exactamente las revisiones indicadas, ejecuta todos los tests y conserva trazas, metadatos, corpus y logs. Para reproducir localmente, colocar ambos proyectos en `external-oracles/pokerkit` y `external-oracles/treys` y ejecutar desde la raíz:

```bash
export PYTHONPATH="$PWD/external-oracles/pokerkit:$PWD/external-oracles/treys"
export SOLVERPK_EXTERNAL_EVALUATOR_TSV=external-evaluator.tsv
export SOLVERPK_EXTERNAL_HANDS_JSONL=external-hands.jsonl
export SOLVERPK_EXTERNAL_HANDS=6000
cargo test --all-targets --no-fail-fast
python scripts/external_evaluator.py --output external-evaluator.tsv
cargo test --release external_evaluator_matches -- --ignored --nocapture
cargo test --release external_engine_export -- --ignored --nocapture
python scripts/external_comparator_tests.py -v
python scripts/external_engine_compare.py --input external-hands.jsonl --output external-comparison.json --expected-hands 6000
cargo test --release exhaustive_five_card -- --ignored --nocapture
cargo test --release audit_stress -- --ignored --nocapture
```

La auditoría no certifica todas las variantes de póker ni los motores heads-up históricos separados. Las comprobaciones de reglas, rentabilidad estratégica y controles de sesión tienen alcances distintos, indicados en este informe.
