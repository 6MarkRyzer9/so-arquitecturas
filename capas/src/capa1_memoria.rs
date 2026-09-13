//! CAPA 1 — Gestión de memoria.
//!
//! Decide qué marcos de RAM pertenecen a qué proceso. Es la ÚNICA capa que
//! puede hablar con el hardware, por lo que también debe "dejar pasar" hacia
//! abajo las operaciones de CPU y disco que pidan las capas superiores.

use crate::capa0_hardware::Hardware;
use crate::traza;

pub type Pid = u32;

#[derive(Debug)]
pub enum ErrorMemoria {
    SinMarcos { pedidos: usize, libres: usize },
    BloqueInexistente(usize),
}

impl std::fmt::Display for ErrorMemoria {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ErrorMemoria::SinMarcos { pedidos, libres } => {
                write!(f, "SinMarcos(pedidos {}, libres {})", pedidos, libres)
            }
            ErrorMemoria::BloqueInexistente(b) => write!(f, "BloqueInexistente({})", b),
        }
    }
}

pub struct GestorMemoria {
    hw: Hardware,                // privado: nadie de arriba puede tocar el hardware
    duenos: Vec<Option<Pid>>,    // marco -> proceso dueño (None = libre)
}

impl GestorMemoria {
    pub fn nuevo(hw: Hardware) -> Self {
        let n = hw.num_marcos();
        GestorMemoria { hw, duenos: vec![None; n] }
    }

    /// Reserva `paginas` marcos para `pid` y los limpia en el hardware.
    pub fn reservar(&mut self, pid: Pid, paginas: usize) -> Result<Vec<usize>, ErrorMemoria> {
        let libres: Vec<usize> = (0..self.duenos.len())
            .filter(|&m| self.duenos[m].is_none())
            .collect();

        if libres.len() < paginas {
            let e = ErrorMemoria::SinMarcos { pedidos: paginas, libres: libres.len() };
            traza::subir(1, &format!("Err({})", e));
            return Err(e);
        }

        let asignados = libres[..paginas].to_vec();
        for &m in &asignados {
            self.duenos[m] = Some(pid);
            traza::bajar(1, &format!("escribir_ram(marco {}, 0)", m));
            self.hw.escribir_ram(m, 0);
        }
        traza::subir(1, &format!("Ok(marcos {:?})", asignados));
        Ok(asignados)
    }

    /// Libera todos los marcos de un proceso. Devuelve cuántos liberó.
    pub fn liberar(&mut self, pid: Pid) -> usize {
        let mut n = 0;
        for d in self.duenos.iter_mut() {
            if *d == Some(pid) {
                *d = None;
                n += 1;
            }
        }
        traza::subir(1, &format!("Ok({} marcos libres de nuevo)", n));
        n
    }

    pub fn escribir(&mut self, marco: usize, valor: u32) {
        traza::bajar(1, &format!("escribir_ram(marco {}, {})", marco, valor));
        self.hw.escribir_ram(marco, valor);
    }

    /// Paso obligado: la capa 2 no puede pedir un ciclo de CPU directamente
    /// al hardware, así que esta capa lo reenvía (costo de la estructura).
    pub fn ciclo_cpu(&mut self) -> u64 {
        traza::bajar(1, "tick()");
        self.hw.tick()
    }

    pub fn reloj(&mut self) -> u64 {
        traza::bajar(1, "reloj()");
        self.hw.reloj()
    }

    /// Lee un bloque de disco y lo deja cargado en un marco del proceso.
    pub fn cargar_bloque(&mut self, bloque: usize, marco: usize) -> Result<String, ErrorMemoria> {
        traza::bajar(1, &format!("leer_bloque({})", bloque));
        let datos = match self.hw.leer_bloque(bloque) {
            Some(d) => d,
            None => {
                let e = ErrorMemoria::BloqueInexistente(bloque);
                traza::subir(1, &format!("Err({})", e));
                return Err(e);
            }
        };
        // Guardamos en RAM un "resumen" del bloque (su largo) como buffer.
        traza::bajar(1, &format!("escribir_ram(marco {}, {})", marco, datos.len()));
        self.hw.escribir_ram(marco, datos.len() as u32);
        let verif = self.hw.leer_ram(marco);
        traza::subir(1, &format!("Ok(\"{}\")  [{} bytes en marco {}]", datos, verif, marco));
        Ok(datos)
    }

    /// (usados, total)
    pub fn uso(&self) -> (usize, usize) {
        let usados = self.duenos.iter().filter(|d| d.is_some()).count();
        traza::subir(1, &format!("Ok({} usados de {})", usados, self.duenos.len()));
        (usados, self.duenos.len())
    }

    /// Copia del mapa de marcos (las capas de arriba reciben una copia,
    /// nunca la estructura interna).
    pub fn mapa(&self) -> Vec<Option<Pid>> {
        self.duenos.clone()
    }
}
