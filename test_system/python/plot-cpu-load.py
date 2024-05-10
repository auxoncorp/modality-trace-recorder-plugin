#!/usr/bin/env python3

import pandas
import json
import numpy as np
import struct
import argparse

import plotly.graph_objects as go
import plotly.express as px
from plotly.subplots import make_subplots

import lib.cli as cli

parser = argparse.ArgumentParser(description='Generate CPU load plot')
parser.add_argument('--html', dest="html_file", metavar='FILE', default=None, help='Generate an HTML file.')
args = parser.parse_args()

results = cli.query('* @ * (_.task != "IDLE" AND exists(_.cpu_utilization)) AS s AGGREGATE distinct(s.task)', normalize_json=False)
tasks = results[0]['value']['Set']

results = cli.query('* @ * (_.task != "IDLE" AND exists(_.cpu_utilization))')
results = results.reindex(columns=[
    'attributes.event.total_runtime.Timestamp',
    'attributes.event.task',
    'attributes.event.cpu_utilization'])
results.columns = ['total_runtime', 'task', 'cpu_utilization']
results.dropna(inplace=True)
results = results.astype({
    'total_runtime': float,
    'cpu_utilization': float,
})

results['total_run_time_sec'] = results.apply(lambda row: row.total_runtime / 1000000000.0, axis=1)
results['cpu_utilization'] = results.apply(lambda row: row.cpu_utilization * 100.0, axis=1)

fig = make_subplots(specs=[[{'secondary_y': False}]])

for task in tasks:
    df = results[results['task'] == task]
    fig.add_trace(go.Scatter(
        x=df['total_run_time_sec'],
        y=df['cpu_utilization'],
        name=task,
        hovertemplate='<br>'.join(['Time: %{x}', 'CPU Load: %{y}'])),
        secondary_y=False,
    )

fig.update_layout(title='Task CPU Load')
fig.update_xaxes(title_text='Time (seconds)')
fig.update_yaxes(title_text='%')

if args.html_file == None:
    fig.show()
else:
    fig.write_html(args.html_file, include_plotlyjs="cdn")
