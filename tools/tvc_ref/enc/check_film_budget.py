#!/usr/bin/env python3
"""Boundary tests for film encoder-slot budget selection."""
import importlib.util
from pathlib import Path
root=Path(__file__).resolve().parents[3]
spec=importlib.util.spec_from_file_location('film', root/'tools/tnv3_ref/encode_film.py')
film=importlib.util.module_from_spec(spec); spec.loader.exec_module(film)
for jobs,budget,threads,want in [(8,4,2,2),(1,4,2,1),(8,4,1,4),(8,None,1,8),(8,5,2,2)]:
 assert film.encoder_schedule(jobs,budget,threads)==want
for args in [(0,None,1),(4,1,2),(4,0,1),(4,None,3)]:
 try:film.encoder_schedule(*args)
 except ValueError:continue
 raise AssertionError(args)
print('PASS film encoder budget boundaries')
