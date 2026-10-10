"""Run Python check-jsonschema on prek's parity fixtures using upstream hook entries."""
import pathlib, shlex, subprocess, sys
import ruamel.yaml
fixtures = pathlib.Path(sys.argv[1])
cli = sys.argv[2]
hooks = {h["id"]: shlex.split(h["entry"]) for h in ruamel.yaml.YAML(typ="safe").load((fixtures / "pre-commit-hooks.yaml").read_text())}
bad = 0
for category, expected in (("positive", 0), ("negative", 1)):
    for case in sorted((fixtures / "parity" / category).glob("*/*")):
        entry = hooks["check-" + case.parent.name]
        result = subprocess.run([cli, *entry[1:], str(case)], capture_output=True, text=True)
        ok = result.returncode == expected
        bad += not ok
        print(f"{'OK ' if ok else 'BAD'} {category:8} {case.parent.name:34} {case.name:24} exit={result.returncode}")
        if not ok:
            print("    " + (result.stdout + result.stderr).strip().replace("\n", "\n    ")[:600])
sys.exit(bad)
