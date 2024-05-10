#!/usr/bin/env python3

import pandas
import json
import numpy as np
import struct
import argparse
import sys

import plotly.graph_objects as go
import plotly.express as px
from plotly.subplots import make_subplots

import lib.cli as cli

parser = argparse.ArgumentParser(description='Generate stack usage plot')
parser.add_argument('--html', dest="html_file", metavar='FILE', default=None, help='Generate an HTML file.')
args = parser.parse_args()

results = cli.query('stats @ * (_.task != "IDLE") AS s AGGREGATE distinct(s.task)', normalize_json=False)
tasks = results[0]['value']['Set']

results = cli.query('stats @ * (_.task != "IDLE")')
results = results.reindex(columns=[
    'attributes.timeline.time_resolution.Timestamp',
    'attributes.event.task',
    'attributes.event.stack_high_water',
    'attributes.event.stack_size',
    'attributes.event.task_run_time',
    'attributes.event.total_run_time'])
results.columns = ['time_resolution', 'task', 'stack_high_water', 'stack_size', 'task_run_time', 'total_run_time']
results.dropna(inplace=True)
results = results.astype({
    'time_resolution': float,
    'stack_high_water': float,
    'stack_size': float,
    'task_run_time': float,
    'total_run_time': float,
})

results['task_stack_usage'] = results.apply(lambda row: (1.0 - (row.stack_high_water / row.stack_size)) * 100, axis=1)
results['total_run_time_sec'] = results.apply(lambda row: (row.total_run_time * row.time_resolution) / 1000000000.0, axis=1)

fig = make_subplots(specs=[[{'secondary_y': False}]])

for task in tasks:
    df = results[results['task'] == task]
    fig.add_trace(go.Bar(
        x=df['total_run_time_sec'],
        y=df['task_stack_usage'],
        name=task,
        hovertemplate='<br>'.join(['Time: %{x}', 'Stack Usage: %{y}'])),
        secondary_y=False,
    )

fig.update_layout(title='Task Stack Usage')
fig.update_xaxes(title_text='Time (seconds)')
fig.update_yaxes(title_text='%')
fig.update_layout(barmode='group')

if args.html_file == None:
    #fig.show(renderer='chrome')
    #fig.show(renderer='firefox')
    fig.show()
else:
    fig.write_html(args.html_file, include_plotlyjs="cdn")
