# Tabular 6-max entrenado desde cero — 10M manos con reglas corregidas

**Fecha:** 2026-11-19. **Entrenamiento verificado:** [GitHub Actions #44601763525](https://github.com/Shisetsu-Code/SolverPK/actions/runs/44601763525), ejecución correcta. Checkpoints en el artefacto `sixmax-tabular-corrected-10m`.

## Condiciones

- Motor 6-max con liquidación auditada, comisión explícita **5 %**, tope **3 BB por mano** y regla no-flop-no-drop.
- Una nueva población de **seis políticas tabulares** `SPK6MAX3`, entrenada desde cero; sin inicialización PHH ni pesos históricos.
- Primer bloque de **5.000.000** manos: seed `2026111901`, learning rate `0.01`.
- Segundo bloque de **5.000.000** manos adicionales: seed `2026111902`, learning rate `0.005`, `--resume`.
- Total de manos simuladas de entrenamiento: **10.000.000**. Promedio observado ~435k–477k manos/s en el runner de GitHub Actions, no hardware local.
- La política tabular mantiene el aprendizaje de `reinforce()` con recompensa/ventaja recortadas. **No** incorpora el nuevo objetivo neuronal lineal V2; sus ganancias de evaluación sí son netas después de rake. No confundir un motor de reglas corregido con un algoritmo de entrenamiento también corregido.

## Evaluación congelada (120k manos por rival y seed)

Cada rival juega en cinco asientos; el candidato usa el asiento 0 con las seis posiciones de botón rotadas. Todos los resultados en **bb/100 netos después de rake**, con intervalos aproximados 95 % basados en grupos de seis manos.

| Checkpoint | Seed | Rival | bb/100 | ± IC95 |
|---|---:|---|---:|---:|
| 5M | 990071 | Aleatorio legal | +140.151 | 40.676 |
| 5M | 990073 | Aleatorio legal | +151.541 | 41.447 |
| 5M | 990071 | Clones de la misma política | −34.308 | 24.977 |
| 5M | 990073 | Clones de la misma política | −25.553 | 24.815 |
| 5M | 990071 | Imitador PHH | +99.040 | 15.801 |
| 5M | 990073 | Imitador PHH | +113.430 | 15.310 |
| **10M** | 990071 | Aleatorio legal | **+129.668** | 38.098 |
| **10M** | 990073 | Aleatorio legal | **+136.221** | 38.964 |
| **10M** | 990071 | Clones de la misma política | **−35.120** | 26.138 |
| **10M** | 990073 | Clones de la misma política | **−31.032** | 26.377 |
| **10M** | 990071 | Imitador PHH | **+119.494** | 16.129 |
| **10M** | 990073 | Imitador PHH | **+137.575** | 15.787 |

**Duelo directo 3 vs. 3**, 120.000 manos, seed `990071` y rake 5 %/3 BB:
- Tabular corregido 10M: **+25.038 ±6.635 bb/100 por jugador**.
- Tabular histórico `scratch.bin`: **−59.001 ±6.649 bb/100 por jugador**.
- Comisión total de mesa: 101.891 bb/100. Ambos resultados no se deben forzar a sumar cero porque hay rake.

## Limitaciones y próximo experimento

- El test `clone` negativo no implica fallo económico; seis clones pagan rake y por tanto su EV neto conjunto es negativo. Es una comprobación distinta de la simetría rake-free.
- Las estimaciones valen para el motor abstracto con rivales internos. PHH es una imitación comprimida, no Pluribus original.
- El entrenamiento tabular utiliza recompensas recortadas y un optimizador viejo, a diferencia de la red neuronal V2. Además, se evaluó un solo agente de los seis entrenados (asiento/política índice 0), no todos.
- La suma de rondas 5M + 5M usa semillas diferentes, sin repetición conocida de la secuencia principal. El checkpoint tabular no registra todos esos metadatos en su formato: se conservan en el workflow y los logs.
- El siguiente estudio sería comparar el nuevo tabular contra la red V1 de 100M y modelos V2 usando una liga multi-rival y semillas adicionales; realizar una ablación de la recompensa tabular sin recortes antes de entrenamientos más grandes.

## Reproducibilidad

```bash
cargo test --all-targets --no-fail-fast
cargo test --release exhaustive_five_card -- --ignored
cargo test --release audit_stress -- --ignored
cargo build --release
./target/release/solverpk --sixmax-train --hands 5000000 --seed 2026111901 --learning-rate 0.01 --rake-bps 500 --rake-cap-bb 3 --checkpoint tabular.bin
./target/release/solverpk --sixmax-train --resume --hands 5000000 --seed 2026111902 --learning-rate 0.005 --rake-bps 500 --rake-cap-bb 3 --checkpoint tabular.bin
```

La receta exacta y los hashes están en [`.github/workflows/tabular-10m-corrected.yml`](.github/workflows/tabular-10m-corrected.yml) y el artefacto del run.
