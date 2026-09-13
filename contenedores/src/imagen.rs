//! IMÁGENES — plantillas de solo lectura formadas por capas.
//!
//! Una imagen NO incluye un kernel: solo los archivos (binarios, librerías,
//! configuración) que el proceso necesita. Las capas se comparten entre
//! imágenes y contenedores (Rc = varias referencias a la misma capa).

use std::collections::BTreeMap;
use std::rc::Rc;

pub struct CapaImagen {
    pub digest: String,
    pub instruccion: String, // la línea del Dockerfile que la creó
    pub archivos: BTreeMap<String, String>,
}

pub struct Imagen {
    pub capas: Vec<Rc<CapaImagen>>, // de abajo (base) hacia arriba
    pub proceso_principal: String,
    pub mem_principal_mb: u32,
    pub puerto: u16,
}

pub struct Registro {
    locales: BTreeMap<String, Imagen>,
    capas: BTreeMap<String, Rc<CapaImagen>>, // caché de capas por digest
}

/// Especificación de una capa en el "Docker Hub" simulado.
type EspecCapa = (&'static str, &'static str, &'static [(&'static str, &'static str)]);

const BASE_DEBIAN: EspecCapa = (
    "sha256:4f4fb700ef54",
    "FROM debian:bookworm-slim",
    &[
        ("/bin/sh", "<binario dash>"),
        ("/etc/os-release", "Debian GNU/Linux 12 (bookworm)"),
        ("/lib/x86_64-linux-gnu/libc.so.6", "<glibc 2.36>"),
    ],
);

fn catalogo_remoto(referencia: &str) -> Option<(Vec<EspecCapa>, &'static str, u32, u16)> {
    match referencia {
        "nginx:1.27" => Some((
            vec![
                BASE_DEBIAN,
                ("sha256:a2abf6c4d29d", "RUN apt-get install nginx", &[("/usr/sbin/nginx", "<binario nginx 1.27>")]),
                ("sha256:e1e2f3a4b5c6", "COPY index.html /usr/share/nginx/html/", &[
                    ("/usr/share/nginx/html/index.html", "<h1>Welcome to nginx!</h1>"),
                    ("/usr/share/nginx/html/50x.html", "<h1>Error del servidor</h1>"),
                    ("/etc/nginx/nginx.conf", "worker_processes 2;"),
                ]),
            ],
            "nginx: master process",
            8,
            80,
        )),
        "postgres:16" => Some((
            vec![
                BASE_DEBIAN,
                ("sha256:9c1d2e3f4a5b", "RUN apt-get install postgresql-16", &[("/usr/lib/postgresql/16/bin/postgres", "<binario postgres 16>")]),
                ("sha256:7b8c9d0e1f2a", "ENV PGDATA=/var/lib/postgresql/data", &[("/var/lib/postgresql/data/PG_VERSION", "16")]),
            ],
            "postgres",
            40,
            5432,
        )),
        _ => None,
    }
}

impl Registro {
    pub fn nuevo() -> Self {
        Registro { locales: BTreeMap::new(), capas: BTreeMap::new() }
    }

    /// Descarga una imagen. Devuelve, por cada capa, (digest, instrucción, ya_existía).
    pub fn pull(&mut self, referencia: &str) -> Result<Vec<(String, String, bool)>, String> {
        let (especs, proceso, mem, puerto) =
            catalogo_remoto(referencia).ok_or(format!("imagen '{}' no encontrada en el registro", referencia))?;
        let mut informe = Vec::new();
        let mut capas = Vec::new();
        for (digest, instr, archivos) in especs {
            let ya_existia = self.capas.contains_key(digest);
            let capa = self
                .capas
                .entry(digest.to_string())
                .or_insert_with(|| {
                    Rc::new(CapaImagen {
                        digest: digest.to_string(),
                        instruccion: instr.to_string(),
                        archivos: archivos.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
                    })
                })
                .clone();
            informe.push((digest.to_string(), instr.to_string(), ya_existia));
            capas.push(capa);
        }
        self.locales.insert(
            referencia.to_string(),
            Imagen {
                capas,
                proceso_principal: proceso.to_string(),
                mem_principal_mb: mem,
                puerto,
            },
        );
        Ok(informe)
    }

    pub fn obtener(&self, referencia: &str) -> Option<&Imagen> {
        self.locales.get(referencia)
    }

    /// Cuántas referencias (imágenes + contenedores) apuntan a cada capa.
    pub fn uso_capas(&self) -> Vec<(String, String, usize)> {
        self.capas
            .values()
            .map(|c| (c.digest.clone(), c.instruccion.clone(), Rc::strong_count(c) - 1))
            .collect()
    }
}
