# Tabular 6-max: continuación 10M → 20M (reglas corregidas)

**Experimento finalizado:** [GitHub Actions 44603187298](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44603187298). Run concluido con éxito. Artefacto `sixmax-tabular-corrected-20m` contiene el modelo final, logs, seeds y verificaciones SHA-256.

## Entrenamiento

- Fuente inicial: `tabular-corrected-10m.bin` del [entrenamiento 10M](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44601763525). Su hash se verificó antes del entrenamiento.
- **10.000.000 manos nuevas** bajo `--resume` con seed `2026111903`, tasa `0.003`.
- **20.000.000 manos totales** al sumar 5M + 5M del checkpoint anterior y 10M de esta continuación.
- Reglas: rake explícito del **5 %**, máximo **3 BB por mano**, no-flop-no-drop. Sin cambios de motor ni algoritmo de optimización tabular.
- Observado en GitHub Actions: 615.880 manos/s durante el bloque nuevo. No es un benchmark del equipo local.
- Verificación de pasos: `CONFIRMED_ADDITIONAL_HANDS=10000000` y `CONFIRMED_CUMULATIVE_HANDS=20000000`; los tests y verificaciones finales terminaron correctamente.
- Advertencia: el optimizador tabular conserva su recompensa y ventaja recortadas; las correcciones de V2 neuronal **no** se aplicaron a este entrenamiento tabular.

## Ganancia neta cada 100 manos

Evaluaciones congeladas, **120.000 manos por semilla/rival y checkpoint**, rotación de botón y tres semillas de evaluación: `990071`, `990073`, `990079`. Unidad: **BB netas ganadas por cada 100 manos** después del rake. Los números individuales incluyen semiancho de IC 95 % aproximado por clusters.

| Modelo | Rival | Seed 990071 | Seed 990073 | Seed 990079 | Media descriptiva |
|---|---|---:|---:|---:|---:|
| 10M | Aleatorio legal | +129.668 ±38.098 | +136.221 ±38.964 | +121.832 ±37.472 | +129.240 |
| **20M** | Aleatorio legal | +118.478 ±36.245 | +119.155 ±36.496 | +111.839 ±35.378 | **+116.491** |
| 10M | PHH simplificado | +119.494 ±16.129 | +137.575 ±15.787 | +129.282 ±15.543 | +128.784 |
| **20M** | PHH simplificado | +93.181 ±14.835 | +102.352 ±14.234 | +95.795 ±14.312 | **+97.109** |
| 10M | Clones de sí mismo | −35.120 ±26.138 | −31.032 ±26.377 | −8.051 ±26.672 | −24.734 |
| **20M** | Clones de sí mismo | −25.789 ±22.034 | −22.972 ±22.308 | −4.069 ±22.279 | **−17.610** |

Las medias son aritméticas descriptivas, **no** estimadores con IC combinado calculado. Los CIs individuales son aproximados, sin ajuste por comparaciones múltiples. El resultado contra clones no es necesariamente cero porque la mesa paga rake.

### Duelo directo: 3 asientos 20M vs 3 asientos 10M

Mismas condiciones de rake y **120.000 manos por seed**. Resultados por jugador de cada equipo en ganancia/pérdida neta **cada 100 manos**:

| Seed | 20M (BB/100 manos) | 10M (BB/100 manos) |
|---|---:|---:|
| 990071 | −15.217 ±4.079 | −13.556 ±4.093 |
| 990073 | −15.759 ±4.096 | −13.293 ±4.111 |
| 990079 | −13.179 ±3.948 | −15.344 ±3.966 |

Ambos equipos pierden rake. No se calculó el intervalo del diferencial emparejado; el duelo **no demuestra** una ventaja robusta de 20M sobre 10M.

## Dictamen

- La continuación **sí se ejecutó** y guardó pesos distintos en un checkpoint final de 20M.
- Ganancia PHH: **+128,78 → +97,11 BB cada 100 manos**, caída de 31,67 BB/100.
- Ganancia aleatorio: **+129,24 → +116,49 BB cada 100 manos**, caída de 12,75 BB/100.
- Contra clones el resultado se acercó a cero: **−24,73 → −17,61 BB/100**; por sí solo esto no acredita mayor fuerza.
- **No promover automáticamente** el 20M por haber visto más manos. Los dos checkpoints son candidatos de estudio; conservar el 10M.
- El rival PHH es una política histórica muy comprimida, no Pluribus real. Los resultados no equivalen a ganancias reales.

**Siguiente mejora justificada:** modificar el objetivo tabular para utilizar recompensas monetarias sin recorte (con ensayo controlado), ligas de rivales diversos y métricas emparejadas; evitar incrementar masivamente las manos del mismo algoritmo sin una señal de mejora.

**Workflow fuente:** [`.github/workflows/tabular-20m-corrected.yml`](.github/workflows/tabular-20m-corrected.yml). Evidencia del experimento y checksum final en el [artefacto de Actions](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44603187298).
