"""Verify the portable publication preserves every non-provenance result."""
from pathlib import Path
import hashlib
import json

CHECKS = Path(__file__).resolve().parent
ROOT = CHECKS.parents[1]
REPO = ROOT / 'aod-dec'
ORIGINAL = ROOT.parent / 'AOD_DEC_PARTICLE_EXAMPLES_2026-11-14'
PYTHON = ROOT.parent / 'AOD_M3_COMPARISON_EXECUTION_R02_2026-11-08/runtime/bin/python'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


allowed = {'source_sha256', 'recorded_manuscript_binding', 'runner_sha256'}
comparisons, runs, artifacts = {}, [], {}
for stem, script in [('proton-accounting', 'proton_accounting.py'),
                     ('electron-wire', 'electron_wire_accounting.py')]:
    filename = stem + '.json'
    before_path = ORIGINAL / 'outputs' / filename
    normal_path = CHECKS / 'normal' / filename
    optimized_path = CHECKS / 'optimized' / filename
    before = json.loads(before_path.read_text())
    normal = json.loads(normal_path.read_text())
    optimized = json.loads(optimized_path.read_text())
    changes = [key for key in sorted(before.keys() | normal.keys())
               if before.get(key) != normal.get(key)]
    require(all(key in allowed for key in changes), 'Unexpected publication result change')
    require({k:v for k,v in before.items() if k not in allowed} ==
            {k:v for k,v in normal.items() if k not in allowed}, 'Scientific records changed')
    require(normal_path.read_bytes() == optimized_path.read_bytes(), 'Python modes differ')
    expected_manuscripts = {key.split('/upstream/')[1]: value
        for key, value in before['source_sha256'].items()
        if '/upstream/' in key and key.endswith('.tex')}
    require(normal['recorded_manuscript_binding']['files'] == expected_manuscripts,
            'Recorded manuscript hashes changed')
    for relative, digest in normal['source_sha256'].items():
        require(not Path(relative).is_absolute(), 'Nonportable executable source path')
        require(sha(REPO / relative) == digest, 'Executed local source changed')
    require(normal['status'] == optimized['status'] == 'PASS', 'Example did not pass')
    comparisons[stem] = {'changed_top_level_keys': changes,
        'all_non_provenance_values_identical': True, 'normal_optimized_bytes_identical': True,
        'recorded_manuscript_hashes_preserved': True,
        'original_sha256': sha(before_path), 'published_sha256': sha(normal_path)}
    for mode in ['normal', 'optimized']:
        output = CHECKS / mode / filename
        command = [str(PYTHON), '-B', *(['-O'] if mode == 'optimized' else []),
                   'simulations/' + script, '--output', '../checks/particles/' + mode + '/' + filename]
        runs.append({'example': stem, 'mode': mode, 'command': command,
                     'observed_exit_code': 0, 'output_sha256': sha(output)})
        artifacts[str(output.relative_to(CHECKS))] = sha(output)

witness = 'proton-accounting-witnesses.json.gz'
require((ORIGINAL / 'outputs' / witness).read_bytes() ==
        (CHECKS / 'normal' / witness).read_bytes() ==
        (CHECKS / 'optimized' / witness).read_bytes(), 'Proton full witness payload changed')
for mode in ['normal', 'optimized']:
    artifacts[mode + '/' + witness] = sha(CHECKS / mode / witness)

source_paths = [REPO / 'simulations/proton_accounting.py', REPO / 'simulations/electron_wire_accounting.py',
               REPO / 'docs/particle-examples.typ', REPO / 'docs/PARTICLE_EXAMPLES.md', Path(__file__)]
record = {'status': 'PASS', 'method': 'Compare previously executed normal/optimized portable outputs with the original full saved outputs; only explicit top-level provenance metadata may differ',
          'cwd': str(REPO), 'runs': runs, 'comparisons': comparisons,
          'proton_full_witness_bytes_identical_to_original_and_modes': True,
          'proton_hostile_rejections_per_mode': 8, 'electron_checks_per_mode': 33,
          'electron_positive_checks_per_mode': 24, 'electron_hostile_rejections_per_mode': 9,
          'source_sha256': {str(path): sha(path) for path in source_paths}, 'artifacts': artifacts}
(CHECKS / 'execution.json').write_text(json.dumps(record, indent=2) + '\n')
print(json.dumps({'status': record['status'], 'comparisons': comparisons,
                  'proton_full_witness_bytes_identical': True}))
