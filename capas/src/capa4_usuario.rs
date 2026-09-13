//! CAPA 4 — Programas de usuario (una pequeña shell).
//!
//! Solo conoce las syscalls de la capa 3. No sabe que existen PCBs, marcos
//! ni discos: recibe números y textos.

use crate::capa1_memoria::Pid;
use crate::capa2_procesos::Estado;
use crate::capa3_syscalls::{InterfazSyscalls, Retorno, Syscall};
use crate::traza;

pub struct Shell {
    so: InterfazSyscalls, // privado: main no puede saltarse la shell
}

impl Shell {
    pub fn nuevo(so: InterfazSyscalls) -> Self {
        Shell { so }
    }

    fn syscall(&mut self, s: Syscall) -> Retorno {
        traza::bajar(4, &format!("syscall {:?}", s));
        self.so.llamar(s)
    }

    pub fn lanzar(&mut self, nombre: &str, paginas: usize, instrucciones: u32) -> Option<Pid> {
        println!("usuario$ lanzar {} --paginas {} --instr {}", nombre, paginas, instrucciones);
        let r = self.syscall(Syscall::CrearProceso { nombre: nombre.into(), paginas, instrucciones });
        match r {
            Retorno::Pid(pid) => {
                println!("  => proceso '{}' creado con PID {}\n", nombre, pid);
                Some(pid)
            }
            Retorno::Error { codigo, nombre: n } => {
                println!("  => ERROR {} ({}): no se pudo crear '{}'\n", codigo, n, nombre);
                None
            }
            _ => None,
        }
    }

    pub fn leer(&mut self, pid: Pid, bloque: usize) {
        println!("usuario$ leer --pid {} --bloque {}", pid, bloque);
        match self.syscall(Syscall::LeerBloque { pid, bloque }) {
            Retorno::Datos(d) => println!("  => \"{}\"\n", d),
            Retorno::Error { codigo, nombre } => println!("  => ERROR {} ({})\n", codigo, nombre),
            _ => {}
        }
    }

    pub fn matar(&mut self, pid: Pid) {
        println!("usuario$ kill {}", pid);
        match self.syscall(Syscall::Terminar { pid }) {
            Retorno::Liberados(n) => println!("  => PID {} terminado, {} marcos liberados\n", pid, n),
            Retorno::Error { codigo, nombre } => println!("  => ERROR {} ({})\n", codigo, nombre),
            _ => {}
        }
    }

    pub fn ps(&mut self) {
        println!("usuario$ ps");
        if let Retorno::Procesos(lista) = self.syscall(Syscall::ListarProcesos) {
            println!("  PID  NOMBRE        ESTADO      PÁGINAS  PROGRESO");
            for p in lista {
                let estado = match p.estado {
                    Estado::Listo => "listo",
                    Estado::Terminado => "terminado",
                };
                println!(
                    "  {:<4} {:<13} {:<11} {:<8} {}/{}",
                    p.pid, p.nombre, estado, p.paginas, p.hechas, p.total
                );
            }
            println!();
        }
    }

    pub fn free(&mut self) {
        println!("usuario$ free");
        if let Retorno::Memoria { usados, total, mapa } = self.syscall(Syscall::UsoMemoria) {
            let dibujo: Vec<String> = mapa
                .iter()
                .map(|m| match m {
                    Some(pid) => format!("{}", pid),
                    None => "·".to_string(),
                })
                .collect();
            println!("  marcos: [{}]", dibujo.join(" "));
            println!("  usados {}/{}  ({} libres)\n", usados, total, total - usados);
        }
    }

    /// Ejecuta rondas de Round Robin hasta que no queden procesos listos
    /// (o hasta `max_rondas`).
    pub fn ejecutar(&mut self, max_rondas: usize) {
        println!("usuario$ ejecutar (Round Robin)");
        for ronda in 1..=max_rondas {
            let eventos = match self.syscall(Syscall::Planificar) {
                Retorno::Ronda(e) => e,
                _ => break,
            };
            if eventos.is_empty() {
                break;
            }
            let resumen: Vec<String> = eventos
                .iter()
                .map(|e| {
                    if e.termino {
                        format!("PID {} {} +{} ✔ fin (libera {} marcos)", e.pid, e.nombre, e.ejecutadas, e.marcos_liberados)
                    } else {
                        format!("PID {} {} +{}", e.pid, e.nombre, e.ejecutadas)
                    }
                })
                .collect();
            println!("  ronda {:>2}: {}", ronda, resumen.join(" | "));
        }
        if let Retorno::Reloj(t) = self.syscall(Syscall::Reloj) {
            println!("  reloj de CPU: {} ciclos\n", t);
        }
    }

    pub fn syscalls_atendidas(&self) -> u64 {
        self.so.atendidas()
    }
}
