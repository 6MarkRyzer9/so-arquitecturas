//! MOTOR DE CONTENEDORES — el equivalente a dockerd.
//!
//! No es parte del kernel: es un programa de usuario que le pide al kernel
//! compartido namespaces, cgroups, montajes y procesos, y con eso "arma"
//! cada contenedor. Imprime salidas parecidas a las de la CLI de Docker.

use crate::contenedor::{Contenedor, Estado, Namespaces};
use crate::imagen::Registro;
use crate::kernel::{log, ErrorKernel, Kernel, TipoNs};

pub struct Opciones {
    pub memoria_mb: u32,
    pub cpus: f32,
}

pub struct Motor {
    kernel: Kernel,
    registro: Registro,
    contenedores: Vec<Contenedor>,
}

/// ID hexadecimal de 64 caracteres (como los de Docker), derivado del nombre.
fn generar_id(nombre: &str) -> String {
    let mut id = String::new();
    for ronda in 0u64..4 {
        let mut h: u64 = 0xcbf29ce484222325 ^ ronda;
        for b in nombre.bytes().chain(ronda.to_le_bytes()) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        id.push_str(&format!("{:016x}", h));
    }
    id
}

impl Motor {
    pub fn nuevo(kernel: Kernel) -> Self {
        Motor { kernel, registro: Registro::nuevo(), contenedores: Vec::new() }
    }

    fn idx(&self, nombre: &str) -> usize {
        self.contenedores
            .iter()
            .position(|c| c.nombre == nombre)
            .unwrap_or_else(|| panic!("no existe el contenedor {}", nombre))
    }

    fn nombre_de_ns(&self, ns: Option<u64>) -> String {
        match ns {
            None => "(host)".to_string(),
            Some(n) => self
                .contenedores
                .iter()
                .find(|c| c.ns.pid == n)
                .map(|c| c.nombre.clone())
                .unwrap_or("?".into()),
        }
    }

    // ───────────────────────────── imágenes ─────────────────────────────

    pub fn pull(&mut self, referencia: &str) {
        println!("$ docker pull {}", referencia);
        match self.registro.pull(referencia) {
            Ok(capas) => {
                for (digest, instr, ya) in capas {
                    let estado = if ya { "Already exists" } else { "Pull complete " };
                    println!("  {}: {}   ← {}", &digest[7..], estado, instr);
                }
                println!("  Status: imagen {} lista (sin kernel: solo archivos)\n", referencia);
            }
            Err(e) => println!("  Error: {}\n", e),
        }
    }

    pub fn capas_compartidas(&self) {
        println!("  Capas de imagen en disco (cada una se guarda UNA vez):");
        println!("    DIGEST        REFERENCIAS  INSTRUCCIÓN");
        for (digest, instr, refs) in self.registro.uso_capas() {
            println!("    {}  {:>6}       {}", &digest[7..], refs, instr);
        }
        println!("  (referencias = imágenes + contenedores que usan esa capa en solo lectura)\n");
    }

    // ─────────────────────────── ciclo de vida ──────────────────────────

    pub fn run(&mut self, nombre: &str, imagen: &str, op: Opciones) -> Result<(), ErrorKernel> {
        println!(
            "$ docker run -d --name {} --network red_app --memory {}m --cpus {} {}",
            nombre, op.memoria_mb, op.cpus, imagen
        );
        let (capas, proceso, mem, puerto) = {
            let img = self.registro.obtener(imagen).expect("imagen no descargada");
            (img.capas.clone(), img.proceso_principal.clone(), img.mem_principal_mb, img.puerto)
        };
        let id = generar_id(nombre);
        let corto = id[..12].to_string();
        let k = &mut self.kernel;
        let o = "dockerd";

        // 1. namespaces: lo que el contenedor podrá VER
        let mut ids = Vec::new();
        for t in [TipoNs::Pid, TipoNs::Mnt, TipoNs::Net, TipoNs::Uts] {
            let n = k.unshare(o, t)?;
            log(&format!("unshare({})  → namespace {}", t.flag(), n));
            ids.push(n);
        }
        let ns = Namespaces { pid: ids[0], mnt: ids[1], net: ids[2], uts: ids[3] };

        // 2. cgroup: lo que el contenedor podrá USAR
        let cg = format!("/docker/{}", corto);
        k.crear_cgroup(o, &cg, op.memoria_mb, op.cpus)?;
        log(&format!("cgroup {}  memory.max={}M  cpu.max={}", cg, op.memoria_mb, op.cpus));

        // 3. sistema de archivos por capas
        log(&format!("mount overlay  lower = {} capas de imagen (ro)  upper = capa rw nueva", capas.len()));

        // 4. red y hostname
        let ip = k.conectar_bridge(o, ns.net)?;
        log(&format!("veth ↔ bridge red_app  → IP {}", ip));
        log(&format!("sethostname(\"{}\") en el namespace UTS {}", corto, ns.uts));

        // 5. el proceso principal: un proceso NORMAL del host, con otra vista
        let (pid_host, pid_local) = k.clone_proceso(o, &proceso, Some(ns.pid), Some(&cg), mem)?;
        log(&format!("clone(\"{}\")  → PID {} en el host  |  PID {} dentro del contenedor", proceso, pid_host, pid_local));

        println!("  {}", id);
        println!("  ✔ '{}' corriendo · kernels arrancados: 0 (reutiliza el del host)\n", nombre);
        self.contenedores
            .push(Contenedor::nuevo(id, nombre, imagen, ns, ip, puerto, cg, pid_host, capas));
        Ok(())
    }

    /// El proceso principal del contenedor crea un proceso hijo.
    pub fn hijo(&mut self, nombre: &str, proceso: &str, mem_mb: u32) -> Result<(), ErrorKernel> {
        let i = self.idx(nombre);
        let (ns, cg) = (self.contenedores[i].ns.pid, self.contenedores[i].cgroup.clone());
        let (ph, pl) = self.kernel.clone_proceso(nombre, proceso, Some(ns), Some(&cg), mem_mb)?;
        println!("  [{}] fork → '{}'  PID host {}  |  PID {} en el contenedor", nombre, proceso, ph, pl);
        Ok(())
    }

    // ────────────────────────────── vistas ──────────────────────────────

    pub fn exec_ps(&mut self, nombre: &str) -> Result<(), ErrorKernel> {
        println!("$ docker exec {} ps", nombre);
        let i = self.idx(nombre);
        let lista = self.kernel.ps(nombre, Some(self.contenedores[i].ns.pid))?;
        println!("    PID  CMD");
        for p in lista {
            println!("    {:<4} {}", p.pid_local, p.nombre);
        }
        println!();
        Ok(())
    }

    pub fn host_ps(&mut self) -> Result<(), ErrorKernel> {
        println!("host$ ps -e   (visto desde el namespace raíz)");
        let lista = self.kernel.ps("host", None)?;
        println!("    PID   CMD                        CONTENEDOR   PID DENTRO");
        for p in lista {
            let dentro = if p.ns_pid.is_some() { p.pid_local.to_string() } else { "-".into() };
            println!("    {:<5} {:<26} {:<12} {}", p.pid, p.nombre, self.nombre_de_ns(p.ns_pid), dentro);
        }
        println!();
        Ok(())
    }

    pub fn identidad(&mut self, nombre: Option<&str>) -> Result<(), ErrorKernel> {
        match nombre {
            Some(n) => {
                let i = self.idx(n);
                let v = self.kernel.uname(n)?;
                let c = &self.contenedores[i];
                println!("$ docker exec {} sh -c 'uname -r; hostname; hostname -i'", n);
                println!("    {}\n    {}\n    {}", v, c.hostname, c.ip);
            }
            None => {
                let v = self.kernel.uname("host")?;
                println!("host$ uname -r; hostname");
                println!("    {}\n    servidor-uah", v);
            }
        }
        Ok(())
    }

    // ──────────────────────── sistema de archivos ───────────────────────

    pub fn escribir(&mut self, nombre: &str, ruta: &str, contenido: &str) -> Result<(), ErrorKernel> {
        println!("$ docker exec {} sh -c 'echo \"{}\" > {}'", nombre, contenido, ruta);
        self.kernel.syscall(nombre)?;
        let i = self.idx(nombre);
        let tipo = self.contenedores[i].escribir(ruta, contenido);
        let expl = if tipo == 'C' {
            "existía en la imagen → se COPIA a la capa rw y se modifica ahí (copy-on-write)"
        } else {
            "archivo nuevo → va directo a la capa rw del contenedor"
        };
        println!("    [{}] {}", tipo, expl);
        Ok(())
    }

    pub fn borrar(&mut self, nombre: &str, ruta: &str) -> Result<(), ErrorKernel> {
        println!("$ docker exec {} rm {}", nombre, ruta);
        self.kernel.syscall(nombre)?;
        let i = self.idx(nombre);
        self.contenedores[i].borrar(ruta);
        println!("    [D] la imagen es de solo lectura → se crea un \"whiteout\" en la capa rw que lo oculta");
        Ok(())
    }

    pub fn leer(&mut self, nombre: &str, ruta: &str) -> Result<(), ErrorKernel> {
        let i = self.idx(nombre);
        println!("$ docker exec {} cat {}      # ns mnt {}", nombre, ruta, self.contenedores[i].ns.mnt);
        self.kernel.syscall(nombre)?;
        match self.contenedores[i].leer(ruta) {
            Some((c, origen)) => println!("    {}      (leído desde: {})", c, origen),
            None => println!("    cat: {}: No such file or directory", ruta),
        }
        Ok(())
    }

    pub fn diff(&self, nombre: &str) {
        println!("$ docker diff {}", nombre);
        for (t, r) in self.contenedores[self.idx(nombre)].diff() {
            println!("    {} {}", t, r);
        }
        println!();
    }

    // ──────────────────────────────── red ───────────────────────────────

    /// Un contenedor se conecta a otro por nombre, a través del bridge.
    pub fn conectar(&mut self, origen: &str, destino: &str, peticion: &str) -> Result<(), ErrorKernel> {
        let io = self.idx(origen);
        let ip_o = self.contenedores[io].ip.clone();
        let destino_c = self.contenedores.iter().find(|c| c.nombre == destino && c.corriendo());
        println!("$ docker exec {} cliente {} \"{}\"", origen, destino, peticion);
        let (ip_d, puerto, imagen) = match destino_c {
            Some(c) => (c.ip.clone(), c.puerto, c.imagen.clone()),
            None => {
                self.kernel.syscall(origen)?;
                println!("    error: Could not resolve host: {}  (el DNS del motor ya no lo conoce)\n", destino);
                return Ok(());
            }
        };
        println!("    DNS interno del motor: {} → {}", destino, ip_d);
        self.kernel.enviar(origen, &ip_o, &ip_d, puerto)?;
        self.kernel.syscall(destino)?; // el servidor atiende la petición
        let respuesta = if imagen.starts_with("postgres") {
            " id | usuario\n    ----+--------\n     1  | henry\n    (1 fila)".to_string()
        } else {
            let id = self.idx(destino);
            self.contenedores[id]
                .leer("/usr/share/nginx/html/index.html")
                .map(|(c, _)| format!("HTTP/1.1 200 OK\n    {}", c))
                .unwrap_or("HTTP/1.1 404".into())
        };
        println!("    {}\n", respuesta);
        Ok(())
    }

    // ───────────────────────────── recursos ─────────────────────────────

    pub fn pedir_memoria(&mut self, nombre: &str, mb: u32, motivo: &str) -> Result<(), ErrorKernel> {
        let i = self.idx(nombre);
        let (pid1, cg, ip) = {
            let c = &self.contenedores[i];
            (c.pid1, c.cgroup.clone(), c.ip.clone())
        };
        println!("  [{}] PID 1 pide {} MB ({})", nombre, mb, motivo);
        match self.kernel.reservar_memoria(nombre, pid1, mb) {
            Ok(()) => {
                let c = self.kernel.cgroup(&cg).unwrap();
                println!("    ✔ concedido: uso del cgroup {}/{} MB\n", c.uso_mem_mb, c.limite_mem_mb);
                Ok(())
            }
            Err(ErrorKernel::OomKill { pid, pedido, limite, uso, muertos }) => {
                log(&format!("OOM en cgroup {}: {} + {} MB > límite {} MB", cg, uso, pedido, limite));
                log(&format!("OOM killer → SIGKILL al PID {} (PID 1 del contenedor)", pid));
                log(&format!("murió el PID 1 del namespace → se eliminan también {:?}", &muertos[1..]));
                self.kernel.desconectar_bridge(&ip);
                self.contenedores[i].estado = Estado::Detenido(137);
                println!("    ✘ '{}' terminó con código 137 (SIGKILL). Los demás contenedores no se enteran.\n", nombre);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub fn docker_ps(&self) {
        println!("$ docker ps -a");
        if self.kernel.en_panico() {
            println!("    Cannot connect to the Docker daemon: el equipo no tiene kernel en ejecución\n");
            return;
        }
        println!("    CONTAINER ID   IMAGEN        ESTADO        IP            NOMBRE");
        for c in &self.contenedores {
            let estado = match c.estado {
                Estado::Corriendo => "Up".to_string(),
                Estado::Detenido(cod) => format!("Exited ({})", cod),
            };
            let ip = if c.corriendo() { c.ip.as_str() } else { "-" };
            println!("    {}   {:<13} {:<13} {:<13} {}", &c.id[..12], c.imagen, estado, ip, c.nombre);
        }
        println!();
    }

    pub fn stats(&self) {
        println!("$ docker stats --no-stream");
        println!("    NOMBRE  MEM USO / LÍMITE   MEM %   CPUS   PIDS   SYSCALLS AL KERNEL");
        for c in self.contenedores.iter().filter(|c| c.corriendo()) {
            let cg = self.kernel.cgroup(&c.cgroup).unwrap();
            let pids = self.kernel.procesos_en(c.ns.pid);
            let sys = self.kernel.syscalls().get(&c.nombre).copied().unwrap_or(0);
            println!(
                "    {:<7} {:>3} MB / {:>3} MB   {:>4.0}%   {:<5}  {:<5}  {}",
                c.nombre,
                cg.uso_mem_mb,
                cg.limite_mem_mb,
                cg.uso_mem_mb as f32 * 100.0 / cg.limite_mem_mb as f32,
                cg.cpus,
                pids,
                sys
            );
        }
        println!();
    }

    pub fn resumen_kernel(&self) {
        let (usada, total) = self.kernel.memoria_host();
        let total_sys: u64 = self.kernel.syscalls().values().sum();
        println!("  Resumen del ÚNICO kernel del host:");
        println!("    kernels en ejecución ........... 1");
        println!("    contenedores creados ........... {}", self.contenedores.len());
        println!("    namespaces creados ............. {}", self.kernel.num_namespaces());
        println!("    cgroups creados ................ {}", self.kernel.num_cgroups());
        println!("    paquetes por el bridge ......... {}", self.kernel.paquetes());
        println!("    memoria del host usada ......... {} / {} MB", usada, total);
        println!("    syscalls atendidas ............. {} en total:", total_sys);
        for (origen, n) in self.kernel.syscalls() {
            println!("        {:<8} {:>3}", origen, n);
        }
        println!();
    }

    pub fn fallo_del_kernel(&mut self, desde: &str) {
        println!("  [{}] ejecuta un programa que explota un bug del kernel (simulado)", desde);
        self.kernel.panic(&format!("fallo provocado desde el contenedor '{}'", desde));
        println!();
        println!("  Estado después del pánico:");
        for c in &self.contenedores {
            let efecto = if !c.corriendo() {
                "ya estaba detenido"
            } else if self.kernel.vivo(c.pid1) {
                "sigue vivo"
            } else {
                "estaba Up → CAÍDO"
            };
            println!("    {:<5} {}", c.nombre, efecto);
        }
        println!("    host  systemd, sshd, dockerd → CAÍDOS");
        match self.kernel.syscall("host") {
            Err(ErrorKernel::Panico) => println!("    cualquier syscall nueva → rechazada: no hay kernel\n"),
            _ => println!(),
        }
    }
}
