#!/usr/bin/env python3

import pandas
from colorama import Fore
from colorama import Style

import lib.cli as cli

results = cli.query('* @ * (_.channel = "error" OR _.channel = "warn" OR _.channel = "info")')

results = results.reindex(columns=[
    'attributes.event.timestamp.Timestamp',
    'attributes.event.channel',
    'attributes.event.formatted_string'])
results.columns = ['timestamp', 'channel', 'msg']
results.sort_values('timestamp', inplace=True)

for idx, row in results.iterrows():
    t = row['timestamp']
    ch = row['channel']
    msg = row['msg']

    if ch == 'error':
        color = Fore.RED
    elif ch == 'warn':
        color = Fore.YELLOW
    else:
        color = Fore.GREEN

    print(f"[{t:012}] {color}{ch}{Style.RESET_ALL}: {msg}")
