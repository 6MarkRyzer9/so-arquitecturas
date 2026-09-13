# Trabajo 1 — Simulación de arquitecturas de SO en Rust

Dos simulaciones de consola, cada una en su propio crate dentro del mismo repositorio:

| Crate | Arquitectura | Rasgo distintivo que se muestra |
|---|---|---|
| `capas/` | Sistema de capas (estilo THE, Dijkstra) | Cada capa solo llama a la inmediatamente inferior y oculta su implementación |
| `contenedores/` | Contenedores (estilo Docker) | Todos los contenedores comparten **un** kernel; se aíslan con namespaces y cgroups |

No usan dependencias externas, solo la biblioteca estándar de Rust.

## Cómo ejecutar

```bash
# desde la carpeta de cada simulación
cd capas        && cargo run
cd contenedores && cargo run

# o desde la raíz del repositorio
cargo run -p capas
cargo run -p contenedores

# con pausa entre escenas (cómodo para sacar capturas)
cargo run -p capas -- --pausa
cargo run -p contenedores -- --pausa

# pruebas
cargo test
```

> En Windows se recomienda Windows Terminal o la terminal de VS Code para que se vean bien los caracteres de los diagramas (─ ▶ ✔).

## Simulación 1: sistema de capas

```
C4 Usuario   ─ shell (lanzar, leer, ps, free, kill, ejecutar)
C3 Syscalls  ─ punto de entrada único; traduce errores a códigos (ENOMEM, ESRCH, EIO)
C2 Procesos  ─ PCB, cola de listos, planificador Round Robin (quantum 2)
C1 Memoria   ─ asignación de 16 marcos; único acceso al hardware
C0 Hardware  ─ RAM, disco y reloj de CPU
```

- **Creación de procesos:** `C4 → C3 → C2 → C1 → C0`. C2 arma el PCB y C1 reserva los marcos.
- **Gestión de recursos:** memoria por marcos y CPU por Round Robin.
- **Comunicación:** llamadas a funciones, siempre hacia la capa inferior. Cada capa guarda la de abajo en un **campo privado**, así que el compilador no deja saltarse capas (`shell.so.procesos.mem.hw` no compila).
- **Traza:** `traza.rs` imprime cada cruce de capa y los cuenta, para medir el costo del diseño.

| Escena | Qué muestra |
|---|---|
| 1 | Construcción de la pila de capas |
| 2 | Creación de 3 procesos con traza completa |
| 3 | Un error de memoria que nace en C1 y sube traducido hasta C4 (`-12 ENOMEM`) |
| 4 | Lectura de disco que atraviesa todas las capas y un PID inválido que C2 frena |
| 5 | Planificación Round Robin, término de procesos y memoria liberada |
| 6 | Se reintenta el proceso que antes falló |
| 7 | Estadísticas: llamadas por capa y cruces por cada operación en hardware |

## Simulación 2: contenedores

```
 web (nginx)   web2 (nginx)   db (postgres)    ← contenedores: procesos + archivos
 ════════ namespaces (PID, MNT, NET, UTS) + cgroups ════════
 │               KERNEL COMPARTIDO del host               │
```

| Archivo | Rol |
|---|---|
| `kernel.rs` | Kernel del host: procesos, namespaces, cgroups, OOM killer, red (bridge) |
| `imagen.rs` | Imágenes por capas de solo lectura; las capas se comparten (`Rc`) |
| `contenedor.rs` | Vista del contenedor y sistema de archivos overlay (copy-on-write) |
| `motor.rs` | "dockerd": arma cada contenedor pidiéndole recursos al kernel |

- **Creación de procesos:** `clone()` dentro de un namespace de PID. El proceso tiene PID 1 dentro del contenedor y otro PID en el host.
- **Gestión de recursos:** cgroups con límite de memoria y CPU. Si un contenedor se excede, el OOM killer actúa solo en su cgroup.
- **Comunicación:** red virtual (bridge) entre namespaces de red, con resolución de nombres del motor.

| Escena | Qué muestra |
|---|---|
| 1 | El host: un kernel y sus procesos |
| 2 | `docker pull` con una capa base reutilizada ("Already exists") |
| 3 | `docker run`: namespaces, cgroup, overlay, IP y `clone()` (se arrancan 0 kernels) |
| 4 | `ps` dentro de cada contenedor (cada uno tiene PID 1) y `ps` del host (se ve todo) |
| 5 | `uname -r` idéntico en todos; hostname e IP distintos |
| 6 | Copy-on-write: modificar (C), agregar (A) y borrar (D) sin tocar la imagen |
| 7 | Comunicación web → db y web2 → web por el bridge |
| 8 | `db` supera su límite: OOM kill, `Exited (137)`; los demás siguen `Up` |
| 9 | `docker stats` y resumen: todas las syscalls las atendió un solo kernel |
| 10 | Kernel panic provocado desde un contenedor: caen todos (punto único de falla) |

## Estructura

```
.
├── Cargo.toml            (workspace)
├── capas/
│   └── src/  main.rs, traza.rs, capa0_hardware.rs … capa4_usuario.rs
└── contenedores/
    └── src/  main.rs, kernel.rs, imagen.rs, contenedor.rs, motor.rs
```

## Referencias

- Tanenbaum, A. (2023). *Modern Operating Systems* (Global ed.), cap. 1: estructura de sistemas operativos.
- Silberschatz, A. et al. (2013). *Operating System Concepts*, 9.ª ed., cap. 2.
- *The Rust Programming Language* — doc.rust-lang.org/book
