//! CAPA 2 — Gestión de procesos y planificación.
//!
//! Crea procesos (con su PCB), les pide memoria a la capa 1 y reparte la CPU
//! con Round Robin. Solo conoce la interfaz pública de la capa 1.

use std::collections::{BTreeMap, VecDeque};

use crate::capa1_memoria::{ErrorMemoria, GestorMemoria, Pid};
use crate::traza;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Estado {
    Listo,
    Terminado,
}

/// Bloque de control de proceso (PCB). Es PRIVADO de esta capa.
struct Pcb {
    nombre: String,
    estado: Estado,
    marcos: Vec<usize>,
    hechas: u32,
    total: u32,
}

/// Lo que las capas superiores pueden ver de un proceso.
#[derive(Clone, Debug)]
pub struct InfoProceso {
    pub pid: Pid,
    pub nombre: String,
    pub estado: Estado,
    pub paginas: usize,
    pub hechas: u32,
    pub total: u32,
}

/// Resultado de darle CPU a un proceso durante un quantum.
#[derive(Clone, Debug)]
pub struct EventoCpu {
    pub pid: Pid,
    pub nombre: String,
    pub ejecutadas: u32,
    pub termino: bool,
    pub marcos_liberados: usize,
}

#[derive(Debug)]
pub enum ErrorProcesos {
    SinMemoria(ErrorMemoria),
    PidInexistente(Pid),
    Disco(ErrorMemoria),
}

impl std::fmt::Display for ErrorProcesos {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ErrorProcesos::SinMemoria(e) => write!(f, "SinMemoria <- {}", e),
            ErrorProcesos::PidInexistente(p) => write!(f, "PidInexistente({})", p),
            ErrorProcesos::Disco(e) => write!(f, "Disco <- {}", e),
        }
    }
}

pub struct GestorProcesos {
    mem: GestorMemoria, // privado: la capa 3 no ve la memoria
    pcbs: BTreeMap<Pid, Pcb>,
    listos: VecDeque<Pid>,
    sig_pid: Pid,
    quantum: u32,
}

impl GestorProcesos {
    pub fn nuevo(mem: GestorMemoria, quantum: u32) -> Self {
        GestorProcesos {
            mem,
            pcbs: BTreeMap::new(),
            listos: VecDeque::new(),
            sig_pid: 1,
            quantum,
        }
    }

    /// Crea un proceso: pide memoria a la capa de abajo y arma su PCB.
    pub fn crear(&mut self, nombre: &str, paginas: usize, instrucciones: u32) -> Result<Pid, ErrorProcesos> {
        let pid = self.sig_pid;
        traza::bajar(2, &format!("reservar(pid {}, {} páginas)", pid, paginas));
        match self.mem.reservar(pid, paginas) {
            Ok(marcos) => {
                self.sig_pid += 1;
                self.pcbs.insert(
                    pid,
                    Pcb { nombre: nombre.to_string(), estado: Estado::Listo, marcos, hechas: 0, total: instrucciones },
                );
                self.listos.push_back(pid);
                traza::subir(2, &format!("Ok(pid {})  [PCB creado, en cola de listos]", pid));
                Ok(pid)
            }
            Err(e) => {
                let err = ErrorProcesos::SinMemoria(e);
                traza::subir(2, &format!("Err({})", err));
                Err(err)
            }
        }
    }

    /// Una vuelta completa de Round Robin sobre la cola de listos.
    pub fn ejecutar_ronda(&mut self) -> Vec<EventoCpu> {
        let mut eventos = Vec::new();
        let turno: Vec<Pid> = self.listos.drain(..).collect();

        for pid in turno {
            let (marcos, pendientes) = {
                let pcb = &self.pcbs[&pid];
                (pcb.marcos.clone(), pcb.total - pcb.hechas)
            };
            let a_ejecutar = pendientes.min(self.quantum);

            for _ in 0..a_ejecutar {
                // cada instrucción necesita CPU y escribe en su memoria,
                // pero todo debe pasar por la capa 1.
                traza::bajar(2, "ciclo_cpu()");
                let t = self.mem.ciclo_cpu();
                let hechas = self.pcbs[&pid].hechas;
                let marco = marcos[hechas as usize % marcos.len()];
                traza::bajar(2, &format!("escribir(marco {}, {})", marco, t));
                self.mem.escribir(marco, t as u32);
                self.pcbs.get_mut(&pid).unwrap().hechas += 1;
            }

            let pcb = self.pcbs.get_mut(&pid).unwrap();
            let termino = pcb.hechas >= pcb.total;
            let mut liberados = 0;
            if termino {
                pcb.estado = Estado::Terminado;
                pcb.marcos.clear();
                traza::bajar(2, &format!("liberar(pid {})", pid));
                liberados = self.mem.liberar(pid);
            } else {
                self.listos.push_back(pid);
            }
            let nombre = self.pcbs[&pid].nombre.clone();
            eventos.push(EventoCpu { pid, nombre, ejecutadas: a_ejecutar, termino, marcos_liberados: liberados });
        }
        eventos
    }

    /// Lee un bloque de disco hacia la memoria del proceso.
    pub fn leer_bloque(&mut self, pid: Pid, bloque: usize) -> Result<String, ErrorProcesos> {
        let marco = match self.pcbs.get(&pid) {
            Some(p) if p.estado != Estado::Terminado => p.marcos[0],
            _ => {
                let e = ErrorProcesos::PidInexistente(pid);
                traza::subir(2, &format!("Err({})", e));
                return Err(e);
            }
        };
        traza::bajar(2, &format!("cargar_bloque({}, marco {})", bloque, marco));
        match self.mem.cargar_bloque(bloque, marco) {
            Ok(d) => {
                traza::subir(2, "Ok(datos)");
                Ok(d)
            }
            Err(e) => {
                let err = ErrorProcesos::Disco(e);
                traza::subir(2, &format!("Err({})", err));
                Err(err)
            }
        }
    }

    /// Termina un proceso antes de tiempo y libera su memoria.
    pub fn terminar(&mut self, pid: Pid) -> Result<usize, ErrorProcesos> {
        match self.pcbs.get_mut(&pid) {
            Some(p) if p.estado != Estado::Terminado => {
                p.estado = Estado::Terminado;
                p.marcos.clear();
            }
            _ => return Err(ErrorProcesos::PidInexistente(pid)),
        }
        self.listos.retain(|&p| p != pid);
        traza::bajar(2, &format!("liberar(pid {})", pid));
        let n = self.mem.liberar(pid);
        traza::subir(2, &format!("Ok({} marcos liberados)", n));
        Ok(n)
    }

    pub fn listar(&self) -> Vec<InfoProceso> {
        traza::subir(2, &format!("Ok({} PCBs, copia de solo lectura)", self.pcbs.len()));
        self.pcbs
            .iter()
            .map(|(&pid, p)| InfoProceso {
                pid,
                nombre: p.nombre.clone(),
                estado: p.estado,
                paginas: p.marcos.len(),
                hechas: p.hechas,
                total: p.total,
            })
            .collect()
    }

    /// (usados, total, mapa de marcos)
    pub fn uso_memoria(&mut self) -> (usize, usize, Vec<Option<Pid>>) {
        traza::bajar(2, "uso()");
        let (u, t) = self.mem.uso();
        (u, t, self.mem.mapa())
    }

    pub fn reloj(&mut self) -> u64 {
        traza::bajar(2, "reloj()");
        self.mem.reloj()
    }
}
