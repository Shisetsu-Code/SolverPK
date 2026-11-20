# Evaluación congelada con rivales que utilizan el board

## Objetivo

Continuar la auditoría de SolverPK comprobando qué ganan los tabulares 10M y 20M frente a rivales independientes de sus pesos y sensibles a las cartas comunitarias. Conservar caja inicial USD 50, stop loss USD 50, exposición máxima USD 10 por mano y ganancias netas cada 100 manos.

## Decisiones de implementación

- Rust sin dependencias nuevas para políticas, simulación y control monetario; Python estándar únicamente para organizar experimentos y resumir registros exactos.
- Reutilizar el motor auditado, el runner congelado de la liga y la contabilidad de sesiones. No entrenar ni modificar checkpoints.
- Añadir un rival `equity_multiway`: simulación de cartas desconocidas sin reemplazo, comparación contra todos los jugadores activos y reparto fraccional de empates. Solo recibe `View`; su azar deriva de información visible y propia, nunca de la semilla del mazo real.
- Reutilizar TAG/LAG existentes como otros dos rivales de diagnóstico. PHH se conserva como control histórico en la condición principal.
- La interfaz de evaluación admite candidatos tabulares y neuronales; el experimento principal compara los dos tabulares solicitados previamente.

## Alternativas consideradas

Reutilizar solo TAG/LAG es más barato, pero sus decisiones confunden categoría absoluta con fuerza relativa. Añadir el estimador de equity introduce una decisión distinta e independiente de los pesos aprendidos. Integrar un bot externo competitivo requeriría otro protocolo y una calibración de fuerza que no están disponibles; no se atribuye ese nivel al nuevo rival.

## Protocolo fijado antes de obtener resultados

- 64 sesiones por candidato/rival/condición; hasta 2.000 manos por sesión.
- Master seed `2026112007`; cada sesión ocupa un bloque no solapado de 2.000 índices. Se comparte el mismo bloque entre candidatos y condiciones para comparación emparejada.
- Inicio del botón `sesión % 6`, con rotación en cada mano; reparto nuevo por mano.
- Condiciones: 100 BB con rake 5%/cap 3 BB; 50 BB con el mismo rake; 100 BB sin rake. No-flop-no-drop.
- BB USD 0,10: entrada de USD 10 o USD 5. El límite máximo permanece USD 10; se reserva la entrada efectiva antes de repartir.
- Rival equity: 96 muestras por decisión, rangos rivales uniformes, parámetros fijos. No es GTO ni una referencia de fuerza humana validada.
- 20 combinaciones: dos modelos × tres rivales × tres condiciones, más dos controles PHH en 100 BB/rake 5%. Máximo programado: 2.560.000 manos; las sesiones detenidas no se reponen.
- Comprobar los bloques previstos contra los intervalos conocidos de entrenamiento/evaluación, incluyendo alias del RNG. La procedencia tabular externa se vincula al SHA-256 del checkpoint; no se afirma conocimiento de exposición histórica no registrada.
- 5.000 remuestreos de sesiones completas, seed de bootstrap `2026112011`. Intervalos aproximados por condición, sin corrección por comparaciones múltiples ni promoción automática de un campeón.

## Salidas y criterio de aceptación

Guardar cada sesión, incluidas las que juegan cero manos. Informar neto entero, manos reales, saldo, drawdown, motivos financieros de detención, finalización del horizonte y capacidad para financiar otra entrada como campos separados.

La tasa principal es `100 * suma(netos) / suma(manos reales)`, en BB y USD. También informar neto medio por sesión, proporción de detenciones e intervalo de incertidumbre. No promediar tasas de sesiones de distinta longitud; cero manos implica tasa no disponible. Las diferencias entre modelos conservan el emparejamiento y sus propios denominadores.

Las tasas corresponden al protocolo con detención, no prueban rentabilidad con bankroll ilimitado ni contra personas. Todos los modelos deben conservar su hash; el dinero debe cumplir `suma(netos) = -rake`; la exposición y el piso de pérdidas deben respetarse con ambos tamaños de stack.

## Riesgos a verificar

Fuga de cartas ocultas, reparto incorrecto de empates multiway, duplicación de cartas simuladas, alteración del mazo por muestreo, solapamiento entre sesiones, denominadores sesgados por detención y reserva incorrecta al reducir el stack. Las escalas de observación neuronal conservan su referencia original de 100 BB.
