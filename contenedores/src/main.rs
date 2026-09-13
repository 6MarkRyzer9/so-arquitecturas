//! Simulación de CONTENEDORES (estilo Docker) sobre un kernel compartido.
//!
//! Rasgo distintivo: todos los contenedores comparten UN SOLO kernel (el del
//! host). Cada contenedor es solo un grupo de procesos normales a los que el
//! kernel les da:
//!   * namespaces → una vista aislada (sus propios PIDs, archivos, red, hostname)
//!   * cgroups    → límites de recursos (memoria, CPU)
//!
//! Módulos:
//!   kernel.rs     el kernel del host (namespaces, cgroups, procesos, red)
//!   imagen.rs     imágenes por capas de solo lectura y el registro
//!   contenedor.rs vista de un contenedor + sistema de archivos copy-on-write
//!   motor.rs      el "dockerd": arma contenedores pidiéndole cosas al kernel
//!
//! Ejecutar:  cargo run            (todo seguido)
//!            cargo run -- --pausa (se detiene entre escenas, útil para capturas)

mod contenedor;
mod imagen;
mod kernel;
mod motor;

use kernel::{ErrorKernel, Kernel};
use motor::{Motor, Opciones};

use std::io::{self, BufRead, Write};

fn escena(n: u32, titulo: &str, pausa: bool) {
    if pausa && n > 1 {
        print!("\n[Enter para continuar]");
        io::stdout().flush().ok();
        let _ = io::stdin().lock().lines().next();
    }
    println!();
    println!("══════════════════════════════════════════════════════════════════════");
    println!(" ESCENA {} · {}", n, titulo);
    println!("══════════════════════════════════════════════════════════════════════");
}

fn main() {
    let pausa = std::env::args().any(|a| a == "--pausa");
    if let Err(e) = demo(pausa) {
        println!("error del kernel: {}", e);
    }
}

fn demo(pausa: bool) -> Result<(), ErrorKernel> {
    // ────────────────────────────────────────────────────────────────
    escena(1, "El host: un kernel y sus procesos", pausa);
    let mut docker = Motor::nuevo(Kernel::arrancar(2048));
    docker.identidad(None)?;
    println!();
    docker.host_ps()?;
    println!("  Diagrama de lo que vamos a construir:");
    println!("    ┌──────────┐ ┌──────────┐ ┌──────────┐");
    println!("    │   web    │ │   web2   │ │    db    │   ← contenedores (solo procesos");
    println!("    │  nginx   │ │  nginx   │ │ postgres │     + archivos, SIN kernel)");
    println!("    └────┬─────┘ └────┬─────┘ └────┬─────┘");
    println!("    ═════╧════════════╧════════════╧═══════  namespaces + cgroups");
    println!("    │      KERNEL COMPARTIDO del host      │");
    println!("    └──────────────────────────────────────┘");

    // ────────────────────────────────────────────────────────────────
    escena(2, "Imágenes: capas de solo lectura que se reutilizan", pausa);
    docker.pull("nginx:1.27");
    docker.pull("postgres:16");
    println!("  La capa base de Debian ya estaba: postgres la reutiliza sin descargarla.");

    // ────────────────────────────────────────────────────────────────
    escena(3, "Crear contenedores: el motor le pide todo al kernel del host", pausa);
    docker.run("web", "nginx:1.27", Opciones { memoria_mb: 64, cpus: 0.5 })?;
    docker.run("web2", "nginx:1.27", Opciones { memoria_mb: 64, cpus: 0.5 })?;
    docker.run("db", "postgres:16", Opciones { memoria_mb: 256, cpus: 1.0 })?;
    println!("  Los procesos principales crean sus hijos (quedan en el mismo namespace):");
    docker.hijo("web", "nginx: worker process", 6)?;
    docker.hijo("web", "nginx: worker process", 6)?;
    docker.hijo("web2", "nginx: worker process", 6)?;
    docker.hijo("db", "postgres: checkpointer", 10)?;
    docker.hijo("db", "postgres: walwriter", 8)?;
    println!();
    docker.capas_compartidas();

    // ────────────────────────────────────────────────────────────────
    escena(4, "Namespace de PID: cada contenedor cree que está solo", pausa);
    docker.exec_ps("web")?;
    docker.exec_ps("db")?;
    println!("  Ambos tienen su propio PID 1. Pero desde el host se ve la verdad:\n");
    docker.host_ps()?;
    println!("  Son procesos normales del MISMO kernel: solo cambia lo que cada uno ve.");

    // ────────────────────────────────────────────────────────────────
    escena(5, "Mismo kernel, distinta identidad (UTS y red)", pausa);
    docker.identidad(Some("web"))?;
    docker.identidad(Some("db"))?;
    docker.identidad(None)?;
    println!("\n  uname -r es IDÉNTICO en todos: no hay SO invitado como en una VM.");
    println!("  hostname e IP cambian porque cada uno tiene su namespace UTS y NET.");

    // ────────────────────────────────────────────────────────────────
    escena(6, "Namespace de montaje + overlay: archivos aislados (copy-on-write)", pausa);
    docker.escribir("web", "/usr/share/nginx/html/index.html", "<h1>Hola desde web</h1>")?;
    docker.escribir("web", "/tmp/sesion.txt", "usuario=henry")?;
    docker.borrar("web", "/usr/share/nginx/html/50x.html")?;
    println!();
    docker.leer("web", "/usr/share/nginx/html/index.html")?;
    docker.leer("web2", "/usr/share/nginx/html/index.html")?;
    docker.leer("db", "/tmp/sesion.txt")?;
    docker.leer("web", "/usr/share/nginx/html/50x.html")?;
    docker.leer("web2", "/usr/share/nginx/html/50x.html")?;
    println!();
    docker.diff("web");
    println!("  web y web2 usan la misma imagen, pero el cambio de web quedó en su");
    println!("  capa propia: la imagen y el otro contenedor no se ven afectados.");

    // ────────────────────────────────────────────────────────────────
    escena(7, "Comunicación: red virtual (bridge) dentro del mismo kernel", pausa);
    docker.conectar("web", "db", "SELECT id, usuario FROM usuarios;")?;
    docker.conectar("web2", "web", "GET / HTTP/1.1")?;
    println!("  Los paquetes nunca salen del equipo: los mueve la pila de red del");
    println!("  kernel compartido entre namespaces de red distintos.");

    // ────────────────────────────────────────────────────────────────
    escena(8, "cgroups: límites de recursos y OOM killer", pausa);
    docker.pedir_memoria("web", 20, "caché de páginas")?;
    docker.pedir_memoria("db", 300, "ordenar una tabla grande")?;
    docker.docker_ps();
    docker.conectar("web", "db", "SELECT 1;")?;
    docker.conectar("web2", "web", "GET / HTTP/1.1")?;
    println!("  El exceso de db se castigó solo en su cgroup: web y web2 siguen Up.");

    // ────────────────────────────────────────────────────────────────
    escena(9, "Estadísticas: todo lo atendió un solo kernel", pausa);
    docker.stats();
    docker.resumen_kernel();

    // ────────────────────────────────────────────────────────────────
    escena(10, "La otra cara: el kernel compartido es un punto único de falla", pausa);
    docker.fallo_del_kernel("web2");
    docker.docker_ps();
    println!("  (En una máquina virtual cada invitado tiene su propio kernel, así que");
    println!("   un fallo como este quedaría encerrado dentro de esa VM.)\n");
    println!("  Aislamiento de contenedores = aislamiento de VISTA, no de kernel.");
    println!("  Ventaja: arrancan sin bootear un SO y comparten capas → livianos.");
    println!("  Desventaja: un fallo (o vulnerabilidad) del kernel afecta a todos.");
    Ok(())
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn motor_con_dos() -> Motor {
        let mut m = Motor::nuevo(Kernel::arrancar(1024));
        m.pull("nginx:1.27");
        m.pull("postgres:16");
        m.run("a", "nginx:1.27", Opciones { memoria_mb: 32, cpus: 0.5 }).unwrap();
        m.run("b", "postgres:16", Opciones { memoria_mb: 64, cpus: 0.5 }).unwrap();
        m
    }

    #[test]
    fn pid_namespace_y_kernel_compartido() {
        let mut k = Kernel::arrancar(1024);
        let ns1 = k.unshare("t", kernel::TipoNs::Pid).unwrap();
        let ns2 = k.unshare("t", kernel::TipoNs::Pid).unwrap();
        let (h1, l1) = k.clone_proceso("t", "p", Some(ns1), None, 0).unwrap();
        let (h2, l2) = k.clone_proceso("t", "q", Some(ns2), None, 0).unwrap();
        assert_eq!((l1, l2), (1, 1)); // ambos se ven como PID 1
        assert_ne!(h1, h2); // pero en el host son distintos
        assert_eq!(k.ps("t", Some(ns1)).unwrap().len(), 1);
        assert!(k.ps("t", None).unwrap().len() >= 5); // el host ve todo
    }

    #[test]
    fn oom_solo_afecta_a_su_cgroup() {
        let mut m = motor_con_dos();
        m.pedir_memoria("b", 500, "prueba").unwrap();
        m.pedir_memoria("a", 4, "prueba").unwrap();
    }

    #[test]
    fn panico_detiene_todo() {
        let mut m = motor_con_dos();
        m.fallo_del_kernel("a");
        assert!(matches!(m.host_ps(), Err(ErrorKernel::Panico)));
    }
}
