//! KERNEL DEL HOST — uno solo, compartido por todos los contenedores.
//!
//! Aquí viven las dos piezas que hacen posible un contenedor en Linux:
//!   * namespaces: cambian lo que un proceso VE (PIDs, archivos, red, hostname)
//!   * cgroups:    limitan lo que un proceso PUEDE USAR (memoria, CPU)
//!
//! El kernel no sabe qué es un "contenedor": solo ve procesos con namespaces
//! y cgroups distintos. El concepto de contenedor lo arma el motor (motor.rs).

use std::collections::BTreeMap;

pub type PidHost = u32;
pub type NsId = u64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TipoNs {
    Pid,
    Mnt,
    Net,
    Uts,
}

impl TipoNs {
    pub fn flag(&self) -> &'static str {
        match self {
            TipoNs::Pid => "CLONE_NEWPID",
            TipoNs::Mnt => "CLONE_NEWNS ",
            TipoNs::Net => "CLONE_NEWNET",
            TipoNs::Uts => "CLONE_NEWUTS",
        }
    }
}

struct Namespace {
    #[allow(dead_code)]
    tipo: TipoNs,
    sig_pid_local: u32, // solo se usa en namespaces de PID
}

#[derive(Clone, Debug)]
pub struct ProcesoHost {
    pub pid: PidHost,
    pub nombre: String,
    pub ns_pid: Option<NsId>, // None = namespace raíz (el host)
    pub pid_local: u32,       // PID que ve el proceso dentro de su namespace
    pub cgroup: Option<String>,
    pub mem_mb: u32,
}

#[derive(Clone, Debug)]
pub struct Cgroup {
    pub limite_mem_mb: u32,
    pub cpus: f32,
    pub uso_mem_mb: u32,
}

#[derive(Debug)]
pub enum ErrorKernel {
    /// El OOM killer mató al proceso. Si era PID 1 de su namespace,
    /// cayeron todos los procesos del namespace.
    OomKill { pid: PidHost, pedido: u32, limite: u32, uso: u32, muertos: Vec<PidHost> },
    SinProceso(PidHost),
    ConexionRechazada(String),
    Panico,
}

impl std::fmt::Display for ErrorKernel {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ErrorKernel::OomKill { pid, pedido, limite, uso, muertos } => write!(
                f,
                "OOM: PID {} pidió {} MB (uso {} / límite {}); procesos eliminados: {:?}",
                pid, pedido, uso, limite, muertos
            ),
            ErrorKernel::SinProceso(p) => write!(f, "no existe el proceso {}", p),
            ErrorKernel::ConexionRechazada(d) => write!(f, "conexión rechazada hacia {}", d),
            ErrorKernel::Panico => write!(f, "kernel en pánico: no atiende syscalls"),
        }
    }
}

pub struct Kernel {
    version: &'static str,
    procesos: BTreeMap<PidHost, ProcesoHost>,
    sig_pid: PidHost,
    namespaces: BTreeMap<NsId, Namespace>,
    sig_ns: NsId,
    cgroups: BTreeMap<String, Cgroup>,
    mem_total_mb: u32,
    syscalls: BTreeMap<String, u64>, // quién le pidió cosas al kernel
    red: BTreeMap<String, NsId>,     // IP -> namespace de red (bridge red_app)
    sig_ip: u8,
    paquetes: u64,
    panico: bool,
}

/// Imprime una línea de traza del kernel.
pub fn log(msg: &str) {
    println!("      [kernel] {}", msg);
}

impl Kernel {
    /// Arranca el kernel del host con algunos procesos propios del sistema.
    pub fn arrancar(mem_total_mb: u32) -> Self {
        let mut k = Kernel {
            version: "6.8.0-generic",
            procesos: BTreeMap::new(),
            sig_pid: 1,
            namespaces: BTreeMap::new(),
            sig_ns: 4026532200, // los namespaces en Linux se identifican por un inodo
            cgroups: BTreeMap::new(),
            mem_total_mb,
            syscalls: BTreeMap::new(),
            red: BTreeMap::new(),
            sig_ip: 2,
            paquetes: 0,
            panico: false,
        };
        for (nombre, mem) in [("systemd", 12), ("sshd", 6), ("dockerd", 40)] {
            k.clone_proceso("host", nombre, None, None, mem).unwrap();
        }
        k.sig_pid = 1040; // el host ya lleva un rato corriendo
        k
    }

    /// Toda operación pedida al kernel pasa por aquí y queda contada
    /// según quién la pidió. Si el kernel está en pánico, nada funciona.
    pub fn syscall(&mut self, origen: &str) -> Result<(), ErrorKernel> {
        if self.panico {
            return Err(ErrorKernel::Panico);
        }
        *self.syscalls.entry(origen.to_string()).or_insert(0) += 1;
        Ok(())
    }

    pub fn uname(&mut self, origen: &str) -> Result<&'static str, ErrorKernel> {
        self.syscall(origen)?;
        Ok(self.version)
    }

    /// unshare(): crea un namespace nuevo.
    pub fn unshare(&mut self, origen: &str, tipo: TipoNs) -> Result<NsId, ErrorKernel> {
        self.syscall(origen)?;
        let id = self.sig_ns;
        self.sig_ns += 1;
        self.namespaces.insert(id, Namespace { tipo, sig_pid_local: 1 });
        Ok(id)
    }

    /// Crea un cgroup con límites de recursos.
    pub fn crear_cgroup(&mut self, origen: &str, nombre: &str, limite_mem_mb: u32, cpus: f32) -> Result<(), ErrorKernel> {
        self.syscall(origen)?;
        self.cgroups.insert(nombre.to_string(), Cgroup { limite_mem_mb, cpus, uso_mem_mb: 0 });
        Ok(())
    }

    /// clone(): crea un proceso, opcionalmente dentro de un namespace de PID
    /// y de un cgroup. Devuelve (PID real en el host, PID dentro del namespace).
    pub fn clone_proceso(
        &mut self,
        origen: &str,
        nombre: &str,
        ns_pid: Option<NsId>,
        cgroup: Option<&str>,
        mem_mb: u32,
    ) -> Result<(PidHost, u32), ErrorKernel> {
        self.syscall(origen)?;
        let pid = self.sig_pid;
        self.sig_pid += 1;
        let pid_local = match ns_pid {
            Some(ns) => {
                let n = self.namespaces.get_mut(&ns).expect("namespace inexistente");
                let local = n.sig_pid_local;
                n.sig_pid_local += 1;
                local
            }
            None => pid,
        };
        self.procesos.insert(
            pid,
            ProcesoHost {
                pid,
                nombre: nombre.to_string(),
                ns_pid,
                pid_local,
                cgroup: cgroup.map(|c| c.to_string()),
                mem_mb: 0,
            },
        );
        if mem_mb > 0 {
            self.reservar_memoria(origen, pid, mem_mb)?;
        }
        Ok((pid, pid_local))
    }

    /// Reserva memoria para un proceso respetando el límite de su cgroup.
    /// Si se pasa del límite, actúa el OOM killer SOLO dentro de ese cgroup.
    pub fn reservar_memoria(&mut self, origen: &str, pid: PidHost, mb: u32) -> Result<(), ErrorKernel> {
        self.syscall(origen)?;
        let cg = match self.procesos.get(&pid) {
            Some(p) => p.cgroup.clone(),
            None => return Err(ErrorKernel::SinProceso(pid)),
        };
        if let Some(nombre) = cg {
            let c = self.cgroups.get_mut(&nombre).unwrap();
            if c.uso_mem_mb + mb > c.limite_mem_mb {
                let (limite, uso) = (c.limite_mem_mb, c.uso_mem_mb);
                let muertos = self.matar(pid);
                return Err(ErrorKernel::OomKill { pid, pedido: mb, limite, uso, muertos });
            }
            c.uso_mem_mb += mb;
        }
        self.procesos.get_mut(&pid).unwrap().mem_mb += mb;
        Ok(())
    }

    /// Mata un proceso. Si es el PID 1 de un namespace, el kernel mata a
    /// todos los procesos de ese namespace (igual que en Linux).
    pub fn matar(&mut self, pid: PidHost) -> Vec<PidHost> {
        let p = match self.procesos.get(&pid) {
            Some(p) => p.clone(),
            None => return vec![],
        };
        let victimas: Vec<PidHost> = match (p.ns_pid, p.pid_local) {
            (Some(ns), 1) => self
                .procesos
                .values()
                .filter(|q| q.ns_pid == Some(ns))
                .map(|q| q.pid)
                .collect(),
            _ => vec![pid],
        };
        for v in &victimas {
            if let Some(q) = self.procesos.remove(v) {
                if let Some(cg) = q.cgroup.and_then(|c| self.cgroups.get_mut(&c)) {
                    cg.uso_mem_mb = cg.uso_mem_mb.saturating_sub(q.mem_mb);
                }
            }
        }
        victimas
    }

    /// ps: lo que se ve depende del namespace desde donde se mira.
    ///  - vista None (host): TODOS los procesos, con su PID real.
    ///  - vista Some(ns):     solo los del namespace, con su PID local.
    pub fn ps(&mut self, origen: &str, vista: Option<NsId>) -> Result<Vec<ProcesoHost>, ErrorKernel> {
        self.syscall(origen)?;
        Ok(self
            .procesos
            .values()
            .filter(|p| vista.is_none() || p.ns_pid == vista)
            .cloned()
            .collect())
    }

    pub fn procesos_en(&self, ns: NsId) -> usize {
        self.procesos.values().filter(|p| p.ns_pid == Some(ns)).count()
    }

    pub fn vivo(&self, pid: PidHost) -> bool {
        !self.panico && self.procesos.contains_key(&pid)
    }

    /// Conecta un namespace de red al bridge docker0 y le asigna una IP.
    pub fn conectar_bridge(&mut self, origen: &str, ns_net: NsId) -> Result<String, ErrorKernel> {
        self.syscall(origen)?;
        let ip = format!("172.18.0.{}", self.sig_ip);
        self.sig_ip += 1;
        self.red.insert(ip.clone(), ns_net);
        Ok(ip)
    }

    pub fn desconectar_bridge(&mut self, ip: &str) {
        self.red.remove(ip);
    }

    /// Envía un paquete por el bridge. El paquete pasa por la pila de red del
    /// MISMO kernel, aunque vaya de un contenedor a otro.
    pub fn enviar(&mut self, origen: &str, ip_origen: &str, ip_destino: &str, puerto: u16) -> Result<(), ErrorKernel> {
        self.syscall(origen)?;
        if !self.red.contains_key(ip_destino) {
            return Err(ErrorKernel::ConexionRechazada(format!("{}:{}", ip_destino, puerto)));
        }
        self.paquetes += 2; // petición + respuesta
        log(&format!("bridge red_app: {} ─▶ {}:{}  (ns red {} ─▶ ns red {})",
            ip_origen, ip_destino, puerto, self.red[ip_origen], self.red[ip_destino]));
        Ok(())
    }

    pub fn cgroup(&self, nombre: &str) -> Option<&Cgroup> {
        self.cgroups.get(nombre)
    }

    pub fn syscalls(&self) -> &BTreeMap<String, u64> {
        &self.syscalls
    }

    pub fn paquetes(&self) -> u64 {
        self.paquetes
    }

    pub fn memoria_host(&self) -> (u32, u32) {
        let usada: u32 = self.procesos.values().map(|p| p.mem_mb).sum();
        (usada, self.mem_total_mb)
    }

    pub fn num_namespaces(&self) -> usize {
        self.namespaces.len()
    }

    pub fn num_cgroups(&self) -> usize {
        self.cgroups.len()
    }

    /// Simula un fallo grave del kernel (p. ej. un bug explotado desde
    /// un contenedor). Como el kernel es compartido, cae TODO.
    pub fn panic(&mut self, motivo: &str) {
        println!("\n      [kernel] *** KERNEL PANIC: {} ***", motivo);
        println!("      [kernel] *** kernel {} detenido: se pierden TODOS los procesos del equipo ***", self.version);
        self.panico = true;
        self.procesos.clear();
    }

    pub fn en_panico(&self) -> bool {
        self.panico
    }
}
