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

parser = argparse.ArgumentParser(description='Generate stack usage plot')
parser.add_argument('--html', dest="html_file", metavar='FILE', default=None, help='Generate an HTML file.')
args = parser.parse_args()

results = cli.query('UNUSED_STACK @ * (_.task != "IDLE") AS s AGGREGATE distinct(s.task)', normalize_json=False)
tasks = results[0]['value']['Set']

results = cli.query('UNUSED_STACK @ * (_.task != "IDLE")')
results = results.reindex(columns=[
    'attributes.event.timestamp.Timestamp',
    'attributes.event.task',
    'attributes.event.low_mark'])
results.columns = ['timestamp', 'task', 'low_mark']
results.dropna(inplace=True)
results = results.astype({
    'timestamp': float,
    'low_mark': float,
})

results['timestamp_sec'] = results.apply(lambda row: row.timestamp / 1000000000.0, axis=1)

fig = make_subplots(specs=[[{'secondary_y': False}]])

for task in tasks:
    df = results[results['task'] == task]
    fig.add_trace(go.Scatter(
        x=df['timestamp_sec'],
        y=df['low_mark'],
        name=task,
        hovertemplate='<br>'.join(['Time: %{x}', 'Low mark: %{y}'])),
        secondary_y=False,
    )

fig.update_layout(title='Task Stack Usage')
fig.update_xaxes(title_text='Time (seconds)')
fig.update_yaxes(title_text='Low mark (bytes)')

if args.html_file == None:
    fig.show()
else:
    fig.write_html(args.html_file, include_plotlyjs="cdn")
