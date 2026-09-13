//! CAPA 0 — Hardware simulado.
//!
//! Es la única capa que "toca" los recursos: RAM, disco y reloj de CPU.
//! No sabe nada de procesos ni de quién es dueño de qué: solo guarda datos.

pub struct Hardware {
    ram: Vec<u32>,       // cada posición es un marco físico
    disco: Vec<String>,  // cada posición es un bloque de disco
    reloj: u64,          // ciclos de CPU transcurridos
}

impl Hardware {
    pub fn nuevo(marcos: usize, disco: Vec<String>) -> Self {
        Hardware { ram: vec![0; marcos], disco, reloj: 0 }
    }

    pub fn num_marcos(&self) -> usize {
        self.ram.len()
    }

    pub fn escribir_ram(&mut self, marco: usize, valor: u32) {
        self.ram[marco] = valor;
    }

    pub fn leer_ram(&self, marco: usize) -> u32 {
        self.ram[marco]
    }

    pub fn leer_bloque(&self, bloque: usize) -> Option<String> {
        self.disco.get(bloque).cloned()
    }

    /// Avanza un ciclo de CPU.
    pub fn tick(&mut self) -> u64 {
        self.reloj += 1;
        self.reloj
    }

    pub fn reloj(&self) -> u64 {
        self.reloj
    }
}
