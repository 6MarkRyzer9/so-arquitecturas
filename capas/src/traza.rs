//! Traza de llamadas entre capas.
//!
//! Cada vez que una capa llama a la de abajo se registra aquí. Así podemos
//! (1) mostrar en pantalla el recorrido de cada operación y
//! (2) contar cuántas fronteras de capa se cruzan (el "costo" del diseño).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub const NOMBRES: [&str; 5] = [
    "C0 Hardware",
    "C1 Memoria",
    "C2 Procesos",
    "C3 Syscalls",
    "C4 Usuario",
];

static VERBOSO: AtomicBool = AtomicBool::new(true);

/// Llamadas RECIBIDAS por cada capa (índice = número de capa).
static RECIBIDAS: [AtomicUsize; 5] = [
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
];

pub fn set_verboso(v: bool) {
    VERBOSO.store(v, Ordering::Relaxed);
}

fn verboso() -> bool {
    VERBOSO.load(Ordering::Relaxed)
}

fn sangria(capa: usize) -> String {
    "    ".repeat(4 - capa)
}

/// La capa `desde` llama a la capa `desde - 1`.
/// Notar que NO existe una función para llamar a una capa arbitraria:
/// el destino siempre es la capa inmediatamente inferior.
pub fn bajar(desde: usize, operacion: &str) {
    let hacia = desde - 1;
    RECIBIDAS[hacia].fetch_add(1, Ordering::Relaxed);
    if verboso() {
        println!("{}C{} ─▶ C{}  {}", sangria(desde), desde, hacia, operacion);
    }
}

/// La capa `desde` devuelve un resultado a la capa `desde + 1`.
pub fn subir(desde: usize, resultado: &str) {
    if verboso() {
        println!("{}C{} ◀─ C{}  {}", sangria(desde + 1), desde + 1, desde, resultado);
    }
}

pub fn recibidas() -> [usize; 5] {
    let mut r = [0; 5];
    for (i, c) in RECIBIDAS.iter().enumerate() {
        r[i] = c.load(Ordering::Relaxed);
    }
    r
}

pub fn total_cruces() -> usize {
    recibidas().iter().sum()
}
