# Evidencia Neural V2 — 19 de noviembre de 2026

Informe: [NEURAL_V2.md](../../../NEURAL_V2.md).

- `results.json`: configuración, hashes, 112 filas de liga y compatibilidad V1.
- `validation.json`: commit, CI, pruebas y contadores.
- `SHA256SUMS`: integridad de los siete checkpoints en `models/`.
- `*-initial.bin`: pesos iniciales; LR cero, sin aprendizaje.
- `*-trained.bin`: piloto de 100k manos por red. No son campeones seleccionados.
- `neural-v1-100m.bin`: modelo histórico inalterado, exclusivamente evaluable.

Las repeticiones de CI usan las mismas semillas y no aumentan la muestra estadística.
