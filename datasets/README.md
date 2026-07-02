# datasets/ — fetch target (contents gitignored)

Datasets are never committed. Each bench directory ships a fetch script that downloads into this
directory and records the content hash of what it fetched, so a result in `results/` can name the
exact bytes it ran against.
