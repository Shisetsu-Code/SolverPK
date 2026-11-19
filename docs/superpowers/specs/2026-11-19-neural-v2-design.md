# Neural v2: observación, objetivo monetario y evaluación

## Encargo y alcance

Continuación autorizada por el usuario de las correcciones priorizadas en `AUDIT_SIXMAX.md`: conservar la identidad de los palos, retirar el recorte de ganancias del aprendizaje, actualizar con una política coherente por mano y ampliar la evaluación fuera de entrenamiento. Se conserva el motor económico auditado y el checkpoint histórico 100M.

Implementación en Rust y biblioteca estándar, sin dependencias nuevas. Las decisiones concretas de arquitectura siguientes son elecciones de implementación; no se atribuyen al usuario.

## Alternativas consideradas

1. Añadir solo cuatro contadores de palo: barato, pero aún pierde la asociación entre rango y palo, tanto en mano como en board. Descartado por información insuficiente.
2. Dos planos de 52 cartas y contexto público compacto: conserva exactamente las cartas conocidas, permite invariancia al orden y mantiene una MLP pequeña. **Elegido**.
3. Transformer/actor-critic/PPO e historial completo: pueden ser útiles después, pero cambiarían simultáneamente representación, algoritmo y capacidad. Se mantienen como trabajo separado.

## Observación y compatibilidad

- Red nueva: **128 → 32 ReLU → 6 softmax enmascarada**.
- Entradas 0..52: cartas propias; 52..104: board visible; 104..128: contexto público normalizado. Sin cartas de rivales ni cartas futuras.
- Cada plano conserva rango y palo conjuntamente y es invariante al orden de sus cartas. No se exige invariancia a renombrar todos los palos: la representación cambia de forma coherente.
- `ObservationVersion::{LegacyV1,SuitAwareV2}` selecciona explícitamente el encoder. Legacy mantiene los primeros 64 valores originales y su orden exacto de operaciones.
- `SPKNN001` continúa cargando y evaluándose, con exactamente 64 términos por neurona. No puede reanudarse para entrenamiento. No se convierte silenciosamente a v2.
- `SPKNN002` identifica dimensiones, versión de observación/optimizador, parámetros y procedencia. Un modelo nuevo requiere entrenamiento nuevo.

## Objetivo y actualización

Recompensa terminal = **neto después de rake / (100 BB)**. La escala es lineal: una pérdida de 100 BB vale −1 y una ganancia de 500 BB vale +5. No se recorta recompensa ni ventaja.

Durante toda la mano se congela la política. Se acumula en f64:

`G = (1 / jugadores) * sum_jugadores sum_decisiones (retorno_propio - baseline_anterior) * grad(log pi(acción|observación))`

Una sola actualización aplica `G` y después actualiza el baseline con la media de los retornos de todos los jugadores, incluyendo quienes no tuvieron decisiones. No se divide por número de decisiones, ni se usa el retorno total de la mesa como etiqueta individual. Esta es una actualización simultánea de agentes independientes con parámetros compartidos; no es el gradiente del beneficio total de la mesa.

Se conserva la proyección de parámetros a [−8,8]. No se añade clipping de norma del gradiente: los retornos del juego ya están acotados físicamente y se usa una escala fija. LR cero conserva pesos, baseline y contadores de actualización.

## Procedencia, guardado y reanudación

- Registrar segmentos de entrenamiento: tipo, seed, índice inicial, cantidad de manos, LR y rake.
- La siguiente ejecución con la misma seed continúa después de todos sus índices ya registrados; botón y reparto dependen del índice absoluto. Dividir una corrida en N y M manos con la misma configuración debe dar el mismo checkpoint que N+M.
- Los segmentos adyacentes equivalentes se fusionan; límites, desbordamientos, longitudes, floats y bytes finales se validan.
- El guardado escribe un temporal, sincroniza y reemplaza el destino. Un fallo no debe truncar el checkpoint anterior.
- La procedencia de modelos legacy y de la destilación desde profesores sin historial completo se etiqueta como no verificada. No se inventan semillas de los datos históricos.

## Liga congelada

CLI `--sixmax-neural-league`: cinco referencias heurísticas explícitas (uniforme legal, calling station, tight-aggressive, loose-aggressive, nit), más checkpoints neuronales y modelos tabular/PHH opcionales.

Cada rival usa los mismos repartos y seis rotaciones de botón. Se informa neto bb/100, comisión de mesa, intervalo aproximado por grupos de reparto, semilla, versión y procedencia. Se informa el peor rival observado sin seleccionar automáticamente un campeón ni presentar el promedio como fuerza real.

Antes de evaluar se contrastan **las semillas reales de reparto** con todos los segmentos conocidos del candidato y los rivales. Semillas maestras distintas no bastan: se detectan colisiones del esquema XOR/multiplicación y la pareja de semillas que comparte estado inicial en el RNG. La ausencia de solapamiento conocido no certifica datos de profesores históricos sin procedencia.

## Aceptación y límites

Pruebas RED→GREEN de las colisiones de cartas y del sesgo por clipping; gradientes contrastados con diferencias finitas; invariancia al orden de visitas; retorno cero global con gradiente propio no nulo; carga legacy reproducible; resume continuo/fragmentado idéntico; evaluación congelada sin solapamiento conocido; toda la auditoría económica anterior sigue pasando.

Se ejecutará un piloto acotado con varias inicializaciones y evaluación separada. No se lanzan otros 100M ni se promete mayor fuerza por haber corregido el algoritmo. La historia completa de apuestas, rangos exactos, actor-critic, GPU y los motores HU antiguos quedan fuera de esta corrección.

Referencia matemática: [Spinning Up — policy optimization y baselines](https://spinningup.openai.com/en/latest/spinningup/rl_intro3.html). El estimador particular y sus pruebas se documentan en el código del proyecto.
