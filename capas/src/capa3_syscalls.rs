//! CAPA 3 — Interfaz de llamadas al sistema.
//!
//! Punto de entrada ÚNICO para los programas de usuario (como una trampa /
//! "trap"). Traduce los errores internos a códigos numéricos estilo errno,
//! de modo que el usuario nunca ve los tipos de las capas de abajo.

use crate::capa1_memoria::Pid;
use crate::capa2_procesos::{ErrorProcesos, EventoCpu, GestorProcesos, InfoProceso};
use crate::traza;

pub const ESRCH: i32 = -3;   // no existe el proceso
pub const EIO: i32 = -5;     // error de E/S
pub const ENOMEM: i32 = -12; // sin memoria

#[derive(Debug)]
pub enum Syscall {
    CrearProceso { nombre: String, paginas: usize, instrucciones: u32 },
    Planificar,
    LeerBloque { pid: Pid, bloque: usize },
    Terminar { pid: Pid },
    ListarProcesos,
    UsoMemoria,
    Reloj,
}

#[derive(Debug)]
pub enum Retorno {
    Pid(Pid),
    Ronda(Vec<EventoCpu>),
    Datos(String),
    Liberados(usize),
    Procesos(Vec<InfoProceso>),
    Memoria { usados: usize, total: usize, mapa: Vec<Option<Pid>> },
    Reloj(u64),
    Error { codigo: i32, nombre: &'static str },
}

pub struct InterfazSyscalls {
    procesos: GestorProcesos, // privado
    atendidas: u64,
}

impl InterfazSyscalls {
    pub fn nuevo(procesos: GestorProcesos) -> Self {
        InterfazSyscalls { procesos, atendidas: 0 }
    }

    pub fn atendidas(&self) -> u64 {
        self.atendidas
    }

    fn traducir(e: ErrorProcesos) -> Retorno {
        let r = match e {
            ErrorProcesos::SinMemoria(_) => Retorno::Error { codigo: ENOMEM, nombre: "ENOMEM" },
            ErrorProcesos::PidInexistente(_) => Retorno::Error { codigo: ESRCH, nombre: "ESRCH" },
            ErrorProcesos::Disco(_) => Retorno::Error { codigo: EIO, nombre: "EIO" },
        };
        if let Retorno::Error { codigo, nombre } = &r {
            traza::subir(3, &format!("return {} ({})", codigo, nombre));
        }
        r
    }

    pub fn llamar(&mut self, s: Syscall) -> Retorno {
        self.atendidas += 1;
        match s {
            Syscall::CrearProceso { nombre, paginas, instrucciones } => {
                traza::bajar(3, &format!("crear(\"{}\", {} pág, {} instr)", nombre, paginas, instrucciones));
                match self.procesos.crear(&nombre, paginas, instrucciones) {
                    Ok(pid) => {
                        traza::subir(3, &format!("return {}", pid));
                        Retorno::Pid(pid)
                    }
                    Err(e) => Self::traducir(e),
                }
            }
            Syscall::Planificar => {
                traza::bajar(3, "ejecutar_ronda()");
                Retorno::Ronda(self.procesos.ejecutar_ronda())
            }
            Syscall::LeerBloque { pid, bloque } => {
                traza::bajar(3, &format!("leer_bloque(pid {}, bloque {})", pid, bloque));
                match self.procesos.leer_bloque(pid, bloque) {
                    Ok(d) => {
                        traza::subir(3, "return datos");
                        Retorno::Datos(d)
                    }
                    Err(e) => Self::traducir(e),
                }
            }
            Syscall::Terminar { pid } => {
                traza::bajar(3, &format!("terminar(pid {})", pid));
                match self.procesos.terminar(pid) {
                    Ok(n) => {
                        traza::subir(3, "return 0");
                        Retorno::Liberados(n)
                    }
                    Err(e) => Self::traducir(e),
                }
            }
            Syscall::ListarProcesos => {
                traza::bajar(3, "listar()");
                let lista = self.procesos.listar();
                traza::subir(3, "return lista");
                Retorno::Procesos(lista)
            }
            Syscall::UsoMemoria => {
                traza::bajar(3, "uso_memoria()");
                let (usados, total, mapa) = self.procesos.uso_memoria();
                traza::subir(2, "Ok(uso + mapa de marcos)");
                traza::subir(3, "return uso");
                Retorno::Memoria { usados, total, mapa }
            }
            Syscall::Reloj => {
                traza::bajar(3, "reloj()");
                Retorno::Reloj(self.procesos.reloj())
            }
        }
    }
}
