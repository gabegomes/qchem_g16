use std::collections::HashMap;
use std::convert::TryInto;
use std::fs::{read_to_string, File};
use std::io::{Error, ErrorKind, Result, Write};
use std::path::Path;
use std::str::FromStr;

const CONV_FACTOR: f64 = 4.46552493159e-4;

fn parse_energy(qchem_out: &str) -> Result<f64> {
    let eline = qchem_out
        .lines()
        .filter(|x| x.starts_with(" The QM part of the energy is"))
        .next();
    // now an Option(str)
    let out = match eline {
        Some(val) => Ok(val),
        None => Err(Error::new(
            ErrorKind::Other,
            "no energy line found in output file",
        )),
    };

    match out {
        Ok(val) => val
            .strip_prefix(" The QM part of the energy is")
            .unwrap()
            .trim()
            .parse::<f64>()
            .map_err(|_| Error::new(ErrorKind::Other, "failed to parse floats")),
        Err(e) => Err(e),
    }
}

fn parse_nums_from_str<T: FromStr>(n: u16, data: String) -> Result<Vec<T>> {
    // Parse a vector of floats from a file.
    let nums: std::result::Result<Vec<_>, _> =
        data.split_whitespace().map(|x| x.parse::<T>()).collect();

    match nums {
        Ok(i) => {
            if i.len() == n.into() {
                Ok(i)
            } else {
                Err(Error::new(
                    ErrorKind::Other,
                    format!("expected {} values, got {}", n, i.len()),
                ))
            }
        }
        Err(_) => Err(Error::new(ErrorKind::Other, "failed to parse values")),
    }
}

pub fn qchem_translate_to_gaussian(
    gaussian_out: &str,
    calc: &Calculation,
    qchem_loc: &Path,
    qchem_out: &Path,
) -> Result<()> {
    let mut outfile = File::create(gaussian_out)?;
    let natoms: u16 = calc.natoms.try_into().unwrap();
    let nder = calc.nder;

    // energy
    let energy = parse_energy(&read_to_string(qchem_out)?)?;
    outfile.write(format!("{:+20.12}", energy).as_bytes())?;

    // dipole
    outfile.write(format!("{:+20.12}{:+20.12}{:+20.12}\n", 0.0, 0.0, 0.0).as_bytes())?;

    // derivatives
    if nder > 0 {
        let mut data = parse_nums_from_str::<f64>(
            3 * natoms,
            read_to_string(Path::new(&qchem_loc).join("efield.dat"))?,
        )?;
        for _ in 0..natoms {
            for el in data.drain(..3) {
                outfile.write(format!("{:+20.12}", el).as_bytes())?;
            }
            outfile.write("\n".as_bytes())?;
        }
        // polarizability + dip derivative (6 + 9 * Natoms)
        for _ in 0..(2 + 3 * natoms) {
            outfile.write(format!("{:+20.12}{:+20.12}{:+20.12}\n", 0.0, 0.0, 0.0).as_bytes())?;
        }
    }

    // hessian
    if nder > 1 {
        let n_hessian = (3 * natoms) * (3 * natoms + 1) / 2;
        let data = parse_nums_from_str::<f64>(
            n_hessian,
            read_to_string(Path::new(&qchem_loc).join("hessian.dat"))?,
        )?;

        // Maybe I should have used fortran. Some annoying indexing going down
        // in the next bit. Don't touch! it works.

        // Basically, the hessian.dat is in Upper triangular, atom first order
        // and the gaussian output is in Lower triangular, coordinate first.
        let mut k = 0;
        let mut hess = HashMap::new();
        for i in 0..(3 * natoms) {
            for j in i..(3 * natoms) {
                hess.insert((i, j), data[k] * CONV_FACTOR);
                hess.insert((j, i), data[k] * CONV_FACTOR);
                k += 1;
            }
        }

        let mut count = 0;
        for i in 0..natoms {
            for ix in 0..3 {
                for j in 0..natoms {
                    for jx in 0..3 {
                        let left = 3 * i + ix;
                        let right = 3 * j + jx;
                        if left >= right {
                            count += 1;
                            outfile.write(format!("{:+20.12}", hess[&(left, right)]).as_bytes())?;
                            if count % 3 == 0 {
                                outfile.write("\n".as_bytes())?;
                            }
                        }
                    }
                }
            }
        }
        outfile.write("\n".as_bytes())?;
    }
    Ok(())
}

#[derive(Debug)]
pub struct Calculation {
    pub natoms: usize,
    pub nder: usize,
    pub charge: i8,
    pub spin: i8,
    pub z: Vec<u8>,
    pub coords: Vec<[f64; 3]>,
}

impl Calculation {
    pub fn get_geometry(&self) -> String {
        let mut output = String::new();
        for i in 0..self.natoms {
            output.push_str(&format!(
                "{}   {}   {}   {}\n",
                self.z[i], self.coords[i][0], self.coords[i][1], self.coords[i][2]
            ));
        }
        output.trim().to_string()
    }
}

pub fn parse_gau_ein(infile: &str) -> Result<Calculation> {
    let gaussfile = read_to_string(infile)?;
    let mut gauss = gaussfile.lines();
    if let Some(header) = gauss.next() {
        // Parse
        let entries = parse_nums_from_str::<i8>(4, header.to_string())?;
        let natoms: usize = entries[0].try_into().unwrap();
        let nder: usize = entries[1].try_into().unwrap();
        let charge: i8 = entries[2];
        let spin: i8 = entries[3];
        let mut coords = Vec::new();
        let mut zvals = Vec::<u8>::new();

        for _ in 0..natoms {
            if let Some(line) = gauss.next() {
                let (start, end) = line.split_at(11);
                let atom = parse_nums_from_str::<u8>(1, start.to_string())?[0];
                let vals = parse_nums_from_str::<f64>(4, end.to_string())?;
                coords.push([vals[0], vals[1], vals[2]]);
                zvals.push(atom);
            } else {
                return Err(Error::new(
                    ErrorKind::Other,
                    "Gaussian input file is truncated",
                ));
            }
        }
        Ok(Calculation {
            natoms: natoms,
            nder: nder,
            charge: charge,
            spin: spin,
            z: zvals,
            coords: coords,
        })
    } else {
        Err(Error::new(ErrorKind::Other, "Gaussian input file is empty"))
    }
}
