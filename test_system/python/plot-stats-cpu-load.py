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

results = cli.query('stats @ * AS s AGGREGATE distinct(s.task)', normalize_json=False)
tasks = results[0]['value']['Set']

results = cli.query('stats @ *')
results = results.reindex(columns=[
    'attributes.event.timestamp.Timestamp',
    'attributes.timeline.time_resolution.Timestamp',
    'attributes.event.task',
    'attributes.event.stack_high_water',
    'attributes.event.task_run_time',
    'attributes.event.total_run_time'])
results.columns = ['timestamp', 'time_resolution', 'task', 'stack_high_water', 'task_run_time', 'total_run_time']
results.dropna(inplace=True)
results = results.astype({
    'timestamp': float,
    'time_resolution': float,
    'stack_high_water': float,
    'task_run_time': float,
    'total_run_time': float,
})

results['timestamp_sec'] = results.apply(lambda row: row.timestamp / 1000000000.0, axis=1)
results['task_cpu_usage'] = results.apply(lambda row: (row.task_run_time / row.total_run_time) * 100.0, axis=1)

fig = make_subplots(specs=[[{'secondary_y': False}]])

for task in tasks:
    df = results[results['task'] == task]
    fig.add_trace(go.Scatter(
        x=df['timestamp_sec'],
        y=df['task_cpu_usage'],
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
