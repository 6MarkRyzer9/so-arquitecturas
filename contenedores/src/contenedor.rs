//! CONTENEDOR — un grupo de procesos del host con su propia "vista".
//!
//! No tiene kernel propio. Tiene:
//!   * namespaces (PID, MNT, NET, UTS) creados en el kernel compartido
//!   * un cgroup con sus límites
//!   * un sistema de archivos por capas (overlay): capas de la imagen en
//!     solo lectura + una capa de escritura propia (copy-on-write)

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::imagen::CapaImagen;
use crate::kernel::{NsId, PidHost};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Estado {
    Corriendo,
    Detenido(i32), // código de salida (137 = matado con SIGKILL)
}

pub struct Namespaces {
    pub pid: NsId,
    pub mnt: NsId,
    pub net: NsId,
    pub uts: NsId,
}

pub struct Contenedor {
    pub id: String,
    pub nombre: String,
    pub imagen: String,
    pub ns: Namespaces,
    pub hostname: String,
    pub ip: String,
    pub puerto: u16,
    pub cgroup: String,
    pub pid1: PidHost,
    pub estado: Estado,
    capas_ro: Vec<Rc<CapaImagen>>,             // compartidas, solo lectura
    capa_rw: BTreeMap<String, Option<String>>, // propia; None = archivo borrado
}

impl Contenedor {
    #[allow(clippy::too_many_arguments)]
    pub fn nuevo(
        id: String,
        nombre: &str,
        imagen: &str,
        ns: Namespaces,
        ip: String,
        puerto: u16,
        cgroup: String,
        pid1: PidHost,
        capas_ro: Vec<Rc<CapaImagen>>,
    ) -> Self {
        let hostname = id[..12].to_string();
        Contenedor {
            id,
            nombre: nombre.to_string(),
            imagen: imagen.to_string(),
            ns,
            hostname,
            ip,
            puerto,
            cgroup,
            pid1,
            estado: Estado::Corriendo,
            capas_ro,
            capa_rw: BTreeMap::new(),
        }
    }

    /// Lectura en overlay: primero la capa propia, luego las de la imagen
    /// de arriba hacia abajo.
    pub fn leer(&self, ruta: &str) -> Option<(String, String)> {
        if let Some(v) = self.capa_rw.get(ruta) {
            return v.clone().map(|c| (c, "capa rw del contenedor".to_string()));
        }
        for capa in self.capas_ro.iter().rev() {
            if let Some(c) = capa.archivos.get(ruta) {
                return Some((c.clone(), format!("capa imagen {}", &capa.digest[7..])));
            }
        }
        None
    }

    /// Escritura copy-on-write: nunca se toca la capa de la imagen.
    /// Devuelve 'A' (agregado) o 'C' (cambiado respecto de la imagen).
    pub fn escribir(&mut self, ruta: &str, contenido: &str) -> char {
        let existia_en_imagen = self.capas_ro.iter().any(|c| c.archivos.contains_key(ruta));
        self.capa_rw.insert(ruta.to_string(), Some(contenido.to_string()));
        if existia_en_imagen { 'C' } else { 'A' }
    }

    pub fn borrar(&mut self, ruta: &str) {
        self.capa_rw.insert(ruta.to_string(), None);
    }

    /// Equivalente a `docker diff`.
    pub fn diff(&self) -> Vec<(char, String)> {
        self.capa_rw
            .iter()
            .map(|(ruta, v)| {
                let en_imagen = self.capas_ro.iter().any(|c| c.archivos.contains_key(ruta));
                let tipo = match (v, en_imagen) {
                    (None, _) => 'D',
                    (Some(_), true) => 'C',
                    (Some(_), false) => 'A',
                };
                (tipo, ruta.clone())
            })
            .collect()
    }

    pub fn corriendo(&self) -> bool {
        self.estado == Estado::Corriendo
    }
}
