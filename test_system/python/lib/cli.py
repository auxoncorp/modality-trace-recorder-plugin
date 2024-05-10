#!/usr/bin/env python3

import pandas
import json
import subprocess
from typing import List, Dict, Any

def modality(args: List[str]) -> str:
    cmd = 'modality'
    try:
        p = subprocess.run(
            [cmd] + args,
            capture_output=True,
            check=True)
    except subprocess.CalledProcessError as e:
        print(e.stdout.decode('utf-8'))
        print(e.stderr.decode('utf-8'))
        raise
    except:
        raise
    return str(p.stdout.decode('utf-8'))

def query(expr, extra_args=[], normalize_json=True, flatten=True):
    args = ['query', '--format', 'json', expr]
    args.extend(extra_args)
    output = modality(args)
    if normalize_json:
        if flatten:
            j = []
            for jline in output.splitlines():
                l = json.loads(jline)
                j.append(l['flattened_labeled_events'][0][0])
            n = pandas.json_normalize(j)
        else:
            j = [json.loads(jline) for jline in output.splitlines()]
            n = pandas.json_normalize(j)
        return n
    else:
        return json.loads(output)
