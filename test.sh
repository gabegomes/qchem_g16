#!/bin/sh

# Test the program by mocking the qchem binary
mkdir -p fake-qchem-dir
cp test-data/* fake-qchem-dir

rundir=$(realpath fake-qchem-dir)
qchem=$(realpath test-data/qchem)
./qchem_g16 -d "$rundir" -e "$qchem"\
            --rem $(realpath test-data/params.rem) \
            1 test-data/Gaussian.EIn Gaussian.EOu Gaussian.Em null null
