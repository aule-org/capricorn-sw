# Physical-safety evaluation

Synthetic conversations replayed against the physical-safety gate (`aule_agent.physical_safety`), to check that the model, the question and the thresholds still give the expected verdicts.

**It calls the real provider (TypeSafe).** A live run costs money and sends the case texts to the provider. It is run by hand, never in CI. The unit tests in `tests/` use fakes and make no calls.

```bash
# Prints what would be run. No calls, no key needed.
uv run python evals/physical-safety/evaluate.py

# Calls the provider. The key is read from stdin, never from an argument.
uv run python evals/physical-safety/evaluate.py --live < key.txt
uv run python evals/physical-safety/evaluate.py --live --cases evals/physical-safety/bilingual-cases.json < key.txt
```

The output is one JSON object per line: a manifest (model, hashes of the question, the gate's source and the case file), one row per check with its verdict and latency, and a summary. The exit code is 0 only when every verdict matched.

| File | What it covers |
|---|---|
| `turn-recovery-cases.json` | A safe change of topic after a refused request, indirect follow-ups to a hazard, ongoing emergencies |
| `bilingual-cases.json` | The same in English and in mixed Spanish and English |
| `prospective-cases.json` | Edge cases written after the first run: holding the object, an adult already handling it |

The cases are synthetic and their labels were written by the people who built the gate. A passing run is a regression check, not an independent safety assessment. Run it again whenever `MODEL`, `QUESTION` or the thresholds change.
