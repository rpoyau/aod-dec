#!/usr/bin/env python3
"""Verify imported execution records and unchanged implementation against its seal."""
import hashlib
import json
from pathlib import Path
import sys

ROOT=Path(__file__).resolve().parents[1]

def checked_path(name):
    path=(ROOT/name).resolve()
    if not path.is_relative_to(ROOT) or not path.is_file():
        raise ValueError('Missing or unsafe bound path: '+name)
    return path

def verify():
    mapping=json.loads((ROOT/'evidence/native-r01/ARTIFACT_MAP.json').read_text())
    imported=json.loads((ROOT/'evidence/pr-import/IMPORT.json').read_text())
    count=0
    for item in mapping['files'].values():
        path=checked_path(item['repository_path'])
        if hashlib.sha256(path.read_bytes()).hexdigest()!=item['sha256']:
            raise ValueError('Archived evidence changed: '+str(path.relative_to(ROOT)))
        count+=1
    source=json.loads((ROOT/'evidence/native-r01/delivery/source-manifest.json').read_text())
    original_digest=hashlib.sha256(json.dumps(source['files'],sort_keys=True,separators=(',',':')).encode()).hexdigest()
    if original_digest!=source['tree_sha256'] or original_digest!=imported['original_candidate_tree_sha256']:
        raise ValueError('Original source-tree binding mismatch')
    # Historical import records remain immutable. Later deliberate implementation
    # changes bind both the original bytes and the new candidate explicitly.
    rebindings_path=ROOT/'evidence/trace-profile/source-rebindings.json'
    rebindings=json.loads(rebindings_path.read_text())['files'] if rebindings_path.exists() else {}
    if not set(rebindings) <= set(imported['unchanged_implementation_files']):
        raise ValueError('Rebinding names a file outside the original implementation seal')
    for name in imported['unchanged_implementation_files']:
        expected=source['files'][name]
        if name in rebindings:
            record=rebindings[name]
            if record.get('before_sha256')!=expected or not record.get('reason'):
                raise ValueError('Rebinding does not preserve original source identity: '+name)
            expected=record['after_sha256']
        if hashlib.sha256(checked_path(name).read_bytes()).hexdigest()!=expected:
            raise ValueError('Implementation differs from imported execution: '+name)
    return {'status':'PASS','archived_records':count,'unchanged_implementation_files':len(imported['unchanged_implementation_files'])-len(rebindings),
            'explicitly_rebound_implementation_files':list(rebindings),
            'original_candidate_tree_sha256':original_digest,
            'scope':'Byte provenance only; execute semantic tests for current compiler correctness.'}

if __name__=='__main__':
    try:
        print(json.dumps(verify(),indent=2))
    except (OSError,ValueError,KeyError,TypeError) as exc:
        print('import provenance: '+str(exc),file=sys.stderr)
        raise SystemExit(1)
