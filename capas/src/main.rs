//! Simulación de un Sistema Operativo por CAPAS.
//!
//! Inspirado en el sistema THE de Dijkstra (Tanenbaum, cap. 1): cada capa
//! solo usa los servicios de la capa inmediatamente inferior y le oculta su
//! implementación a la capa superior.
//!
//!   C4 Usuario   ─ shell y programas
//!   C3 Syscalls  ─ interfaz única de llamadas al sistema
//!   C2 Procesos  ─ PCBs y planificador Round Robin
//!   C1 Memoria   ─ asignación de marcos (y paso obligado al hardware)
//!   C0 Hardware  ─ RAM, disco y reloj
//!
//! Ejecutar:  cargo run            (todo seguido)
//!            cargo run -- --pausa (se detiene entre escenas, útil para capturas)

mod capa0_hardware;
mod capa1_memoria;
mod capa2_procesos;
mod capa3_syscalls;
mod capa4_usuario;
mod traza;

use capa0_hardware::Hardware;
use capa1_memoria::GestorMemoria;
use capa2_procesos::GestorProcesos;
use capa3_syscalls::InterfazSyscalls;
use capa4_usuario::Shell;

use std::io::{self, BufRead, Write};

fn escena(n: u32, titulo: &str, pausa: bool) {
    if pausa && n > 1 {
        print!("\n[Enter para continuar]");
        io::stdout().flush().ok();
        let _ = io::stdin().lock().lines().next();
    }
    println!();
    println!("══════════════════════════════════════════════════════════════════");
    println!(" ESCENA {} · {}", n, titulo);
    println!("══════════════════════════════════════════════════════════════════");
}

fn main() {
    let pausa = std::env::args().any(|a| a == "--pausa");

    // ────────────────────────────────────────────────────────────────
    escena(1, "Arranque: las capas se apilan de abajo hacia arriba", pausa);

    let disco = vec![
        "boot: cargador del SO".to_string(),
        "config: quantum=2".to_string(),
        "notas.txt: entregar Trabajo 1".to_string(),
        "main.rs: fn main() {}".to_string(),
    ];
    // Cada capa se construye ENCIMA de la anterior y se queda con ella como
    // campo privado. Así, por diseño, nadie puede saltarse una capa.
    let c0 = Hardware::nuevo(16, disco);
    let c1 = GestorMemoria::nuevo(c0);
    let c2 = GestorProcesos::nuevo(c1, 2);
    let c3 = InterfazSyscalls::nuevo(c2);
    let mut shell = Shell::nuevo(c3);

    // Lo siguiente NO compila, y es justamente la idea:
    //     shell.so.procesos.mem.hw.escribir_ram(0, 99);
    //     error[E0616]: field `so` of struct `Shell` is private

    println!("  ┌──────────────┬─────────────────────────────────┐");
    for (i, nombre) in traza::NOMBRES.iter().enumerate().rev() {
        let desc = match i {
            4 => "shell y programas de usuario",
            3 => "punto de entrada único (trap)",
            2 => "PCBs + planificador Round Robin",
            1 => "asignación de 16 marcos de RAM",
            _ => "RAM, disco (4 bloques), reloj",
        };
        println!("  │ {:<12} │ {:<31} │", nombre, desc);
        if i > 0 {
            println!("  ├──────────────┼─────────────────────────────────┤  ▼ llama solo a C{}", i - 1);
        }
    }
    println!("  └──────────────┴─────────────────────────────────┘");
    println!("  Regla: la capa N solo puede llamar a la capa N-1.\n");

    // ────────────────────────────────────────────────────────────────
    escena(2, "Creación de procesos (cada llamada baja capa por capa)", pausa);
    let editor = shell.lanzar("editor", 3, 5).unwrap();
    let _compilador = shell.lanzar("compilador", 5, 8);
    let _navegador = shell.lanzar("navegador", 4, 3);
    shell.free();

    // ────────────────────────────────────────────────────────────────
    escena(3, "Error que sube: no hay memoria suficiente", pausa);
    println!("  Quedan 4 marcos libres y 'juego' pide 6. El error nace en C1 y");
    println!("  cada capa lo traduce a su propio lenguaje antes de subirlo.\n");
    shell.lanzar("juego", 6, 4);

    // ────────────────────────────────────────────────────────────────
    escena(4, "Lectura de disco: la petición atraviesa todas las capas", pausa);
    let antes = traza::total_cruces();
    shell.leer(editor, 2);
    let cruces = traza::total_cruces() - antes;
    println!("  Llamadas entre capas para esta lectura: {}  (C4→C3→C2→C1→C0 + escritura del buffer)", cruces);
    println!("  El trabajo real (leer_bloque) lo hizo solo C0; C3, C2 y C1 validaron");
    println!("  y reenviaron la petición. Ese es el overhead del diseño por capas.\n");
    shell.leer(99, 0); // PID que no existe: C2 lo rechaza, nunca llega al hardware

    // ────────────────────────────────────────────────────────────────
    escena(5, "Planificación Round Robin (quantum = 2)", pausa);
    println!("  (traza detallada desactivada para no llenar la pantalla;");
    println!("   los cruces se siguen contando)\n");
    traza::set_verboso(false);
    let antes = traza::total_cruces();
    shell.ejecutar(10);
    let cruces_rr = traza::total_cruces() - antes;
    shell.ps();
    shell.free();
    println!("  Cruces de capa durante la planificación: {}", cruces_rr);
    traza::set_verboso(true);

    // ────────────────────────────────────────────────────────────────
    escena(6, "Ahora sí hay memoria: se reintenta 'juego'", pausa);
    let juego = shell.lanzar("juego", 6, 4);
    if let Some(pid) = juego {
        shell.matar(pid);
    }

    // ────────────────────────────────────────────────────────────────
    escena(7, "Estadísticas: cuánto cuesta respetar las capas", pausa);
    let r = traza::recibidas();
    println!("  Llamadas recibidas por cada capa:");
    let max = *r.iter().max().unwrap_or(&1) as f64;
    for i in (0..5).rev() {
        let barra = "█".repeat(((r[i] as f64 / max) * 30.0).round() as usize);
        let nota = if i == 4 { "(nadie está sobre el usuario)" } else { "" };
        println!("    {:<12} {:>5}  {}{}", traza::NOMBRES[i], r[i], barra, nota);
    }
    let total = traza::total_cruces();
    let sys = shell.syscalls_atendidas();
    println!();
    println!("  Syscalls atendidas por C3 ........ {}", sys);
    println!("  Fronteras de capa cruzadas ....... {}", total);
    println!("  Operaciones sobre el hardware .... {}", r[0]);
    println!(
        "  => Por cada operación en hardware hubo {:.2} cruces de capa en total.",
        total as f64 / r[0].max(1) as f64
    );
    println!("\n  Ventaja observada: cada capa se puede entender y probar por separado,");
    println!("  y un error (ENOMEM, ESRCH) se detiene en la capa que lo detecta.");
    println!("  Desventaja observada: todo pasa por todas las capas, aunque solo");
    println!("  reenvíen la petición (ej. tick() y reloj() atraviesan C1 sin hacer nada).");
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use capa3_syscalls::{Retorno, Syscall, ENOMEM, ESRCH};

    fn so(marcos: usize) -> InterfazSyscalls {
        let c0 = Hardware::nuevo(marcos, vec!["bloque0".into()]);
        InterfazSyscalls::nuevo(GestorProcesos::nuevo(GestorMemoria::nuevo(c0), 2))
    }

    #[test]
    fn el_error_de_memoria_llega_como_enomem() {
        let mut s = so(4);
        let r = s.llamar(Syscall::CrearProceso { nombre: "grande".into(), paginas: 5, instrucciones: 1 });
        assert!(matches!(r, Retorno::Error { codigo: ENOMEM, .. }));
    }

    #[test]
    fn un_pid_invalido_no_llega_al_hardware() {
        let mut s = so(4);
        let r = s.llamar(Syscall::LeerBloque { pid: 42, bloque: 0 });
        assert!(matches!(r, Retorno::Error { codigo: ESRCH, .. }));
    }

    #[test]
    fn round_robin_termina_y_libera_memoria() {
        let mut s = so(8);
        s.llamar(Syscall::CrearProceso { nombre: "a".into(), paginas: 4, instrucciones: 3 });
        s.llamar(Syscall::CrearProceso { nombre: "b".into(), paginas: 4, instrucciones: 5 });
        for _ in 0..5 {
            s.llamar(Syscall::Planificar);
        }
        match s.llamar(Syscall::UsoMemoria) {
            Retorno::Memoria { usados, .. } => assert_eq!(usados, 0),
            _ => panic!("retorno inesperado"),
        }
    }
}
