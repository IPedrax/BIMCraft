//! A project's language: the words its drawings print (a stair's UP, a title block's SCALE, a
//! schedule's headings) and the names commands give what they make (levels, views, sheets, rooms).
//! English, Brazilian Portuguese or Spanish. Names already given stay as they are; the core's
//! messages, which agents read, stay English.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "pt")]
    Portuguese,
    #[serde(rename = "es")]
    Spanish,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::English, Language::Portuguese, Language::Spanish];

    /// Its code in commands and files: en, pt, es.
    pub fn code(self) -> &'static str {
        self.of(["en", "pt", "es"])
    }

    /// Its name, in itself.
    pub fn label(self) -> &'static str {
        self.of(["English", "Português (Brasil)", "Español"])
    }

    /// Of one thing in each language, English first, this language's.
    pub fn of<T>(self, [english, portuguese, spanish]: [T; 3]) -> T {
        match self {
            Language::English => english,
            Language::Portuguese => portuguese,
            Language::Spanish => spanish,
        }
    }

    /// A language by its code or its name, or from a locale as systems give one: pt-BR, es_419.UTF-8.
    pub fn from_code(code: &str) -> Option<Language> {
        let code = code.trim().to_lowercase();
        Some(match code.split(['-', '_', '.']).next().unwrap_or_default() {
            "en" | "english" => Language::English,
            "pt" | "portuguese" | "português" => Language::Portuguese,
            "es" | "spanish" | "español" => Language::Spanish,
            _ => return None,
        })
    }

    /// `english`, a word or a name the core writes, in this language.
    pub fn word(self, english: &str) -> &str {
        self.translated(english).unwrap_or(english)
    }

    /// `english` in this language, when it isn't English and the core's words have it.
    pub fn translated(self, english: &str) -> Option<&'static str> {
        if self == Language::English {
            return None;
        }
        WORDS.iter().find(|w| w[0] == english).map(|&w| self.of(w))
    }

    /// Which of `options` (English words) `name` is, in English or in this language.
    pub fn find(self, name: &str, options: &[&'static str]) -> Option<&'static str> {
        let name = name.trim();
        options.iter().copied().find(|o| o.eq_ignore_ascii_case(name) || self.word(o).to_lowercase() == name.to_lowercase())
    }
}

/// The core's words in English, Brazilian Portuguese and Spanish.
const WORDS: &[[&str; 3]] = &[
    // The line patterns and line styles a project starts with.
    ["Dash", "Tracejado", "Trazos"],
    ["Long Dash", "Traço longo", "Trazo largo"],
    ["Dot", "Pontilhado", "Puntos"],
    ["Dash Dot", "Traço e ponto", "Trazo y punto"],
    ["Hidden", "Oculta", "Oculta"],
    ["Centre", "Eixo", "Eje"],
    ["Hairline", "Linha capilar", "Línea capilar"],
    ["Thin Lines", "Linhas finas", "Líneas finas"],
    ["Medium Lines", "Linhas médias", "Líneas medias"],
    ["Wide Lines", "Linhas grossas", "Líneas gruesas"],
    ["Hidden Lines", "Linhas ocultas", "Líneas ocultas"],
    ["Solid fill", "Preenchimento sólido", "Relleno sólido"],
    ["Diagonal Up", "Diagonal para cima", "Diagonal ascendente"],
    ["Diagonal Down", "Diagonal para baixo", "Diagonal descendente"],
    ["Diagonal Crosshatch", "Hachura cruzada diagonal", "Sombreado cruzado diagonal"],
    ["Crosshatch", "Hachura cruzada", "Sombreado cruzado"],
    ["Horizontal", "Horizontal", "Horizontal"],
    ["Vertical", "Vertical", "Vertical"],
    ["Sand", "Areia", "Arena"],
    ["Brick 225 x 75", "Tijolo 225 x 75", "Ladrillo 225 x 75"],
    ["Tile 300", "Ladrilho 300", "Baldosa 300"],
    ["Solid Black", "Preto sólido", "Negro sólido"],
    ["Masking", "Máscara", "Máscara"],
    ["Centre Lines", "Linhas de eixo", "Líneas de eje"],
    ["Overhead Lines", "Linhas superiores", "Líneas superiores"],
    // Phases and the phase filters a project starts with.
    ["Existing", "Existente", "Existente"],
    ["New Construction", "Nova construção", "Nueva construcción"],
    ["Show All", "Mostrar tudo", "Mostrar todo"],
    ["Show Complete", "Mostrar completo", "Mostrar completo"],
    ["Show New", "Mostrar novo", "Mostrar nuevo"],
    ["Show Previous + Demolished", "Mostrar anterior + demolido", "Mostrar anterior + demolido"],
    ["Show Previous + New", "Mostrar anterior + novo", "Mostrar anterior + nuevo"],
    // Names commands give.
    ["Project", "Projeto", "Proyecto"],
    ["Level", "Nível", "Nivel"],
    ["Section", "Corte", "Sección"],
    ["Elevation", "Elevação", "Elevación"],
    ["North", "Norte", "Norte"],
    ["South", "Sul", "Sur"],
    ["East", "Leste", "Este"],
    ["West", "Oeste", "Oeste"],
    ["Unnamed", "Sem nome", "Sin nombre"],
    ["Room", "Ambiente", "Habitación"],
    ["Mass", "Massa", "Masa"],
    ["Room Schedule", "Tabela de ambientes", "Tabla de habitaciones"],
    ["Door Schedule", "Tabela de portas", "Tabla de puertas"],
    ["Window Schedule", "Tabela de janelas", "Tabla de ventanas"],
    ["Wall Schedule", "Tabela de paredes", "Tabla de muros"],
    ["Floor Schedule", "Tabela de pisos", "Tabla de pisos"],
    ["Level Schedule", "Tabela de níveis", "Tabla de niveles"],
    ["Ceiling Plan", "Planta de forro", "Plano de cielo raso"],
    ["Copy", "Cópia", "Copia"],
    ["Dependent", "Dependente", "Dependiente"],
    // The materials library a new project has.
    ["Brick", "Tijolo", "Ladrillo"],
    ["Insulation", "Isolamento", "Aislamiento"],
    ["Concrete block", "Bloco de concreto", "Bloque de hormigón"],
    ["Plaster", "Reboco", "Enlucido"],
    ["Concrete", "Concreto", "Hormigón"],
    ["Screed", "Contrapiso", "Recrecido"],
    ["Timber", "Madeira", "Madera"],
    ["Gypsum board", "Placa de gesso", "Placa de yeso"],
    ["Ceramic tile", "Cerâmica", "Baldosa cerámica"],
    ["Roof tiles", "Telhas", "Tejas"],
    ["Membrane", "Manta impermeável", "Lámina impermeable"],
    ["Fabric", "Tecido", "Tela"],
    ["Porcelain", "Porcelana", "Porcelana"],
    // The families a project starts with, and their parameters.
    ["Table", "Mesa", "Mesa"],
    ["Chair", "Cadeira", "Silla"],
    ["Bed", "Cama", "Cama"],
    ["Sofa", "Sofá", "Sofá"],
    ["Desk", "Escrivaninha", "Escritorio"],
    ["Wardrobe", "Guarda-roupa", "Armario"],
    ["Base Cabinet", "Armário inferior", "Mueble bajo"],
    ["WC", "Vaso sanitário", "Inodoro"],
    ["Washbasin", "Lavatório", "Lavabo"],
    ["Bathtub", "Banheira", "Bañera"],
    // The fixtures of the discipline tabs.
    ["Kitchen Sink", "Pia de cozinha", "Fregadero de cocina"],
    ["Shower", "Chuveiro", "Ducha"],
    ["Floor Drain", "Ralo", "Sumidero de piso"],
    ["Elbow 90", "Joelho 90", "Codo 90"],
    ["Tee", "Tê", "Te"],
    ["Gate Valve", "Registro de gaveta", "Válvula de compuerta"],
    ["Diameter", "Diâmetro", "Diámetro"],
    ["Distribution Board", "Quadro de distribuição", "Cuadro de distribución"],
    ["Ceiling Luminaire", "Luminária de teto", "Luminaria de techo"],
    ["Recessed Luminaire", "Luminária embutida", "Luminaria empotrada"],
    ["Outlet", "Tomada", "Tomacorriente"],
    ["Switch", "Interruptor", "Interruptor"],
    ["Mounting Height", "Altura de instalação", "Altura de montaje"],
    ["Air Terminal", "Captor", "Terminal aéreo"],
    ["Earthing Electrode", "Haste de aterramento", "Electrodo de puesta a tierra"],
    ["Inspection Box", "Caixa de inspeção", "Caja de inspección"],
    ["Data Outlet", "Ponto de dados", "Punto de datos"],
    ["Voice Outlet", "Ponto de voz", "Punto de voz"],
    ["Patch Panel", "Patch panel", "Panel de parcheo"],
    ["Rack", "Rack", "Rack"],
    ["Single Door", "Porta simples", "Puerta simple"],
    ["Double Door", "Porta dupla", "Puerta doble"],
    ["Fixed Window", "Janela fixa", "Ventana fija"],
    ["Casement Window", "Janela de abrir", "Ventana abatible"],
    ["Frame Width", "Largura do batente", "Ancho del marco"],
    ["Frame Depth", "Profundidade do batente", "Profundidad del marco"],
    ["Frame Material", "Material do batente", "Material del marco"],
    ["Leaf Material", "Material da folha", "Material de la hoja"],
    ["Leaf Thickness", "Espessura da folha", "Espesor de la hoja"],
    ["Glass", "Vidro", "Vidrio"],
    ["Metal", "Metal", "Metal"],
    ["PVC", "PVC", "PVC"],
    ["Copper", "Cobre", "Cobre"],
    ["Depth", "Profundidade", "Profundidad"],
    ["Seat Height", "Altura do assento", "Altura del asiento"],
    ["Leg Width", "Largura da perna", "Ancho de la pata"],
    ["Arm Width", "Largura do braço", "Ancho del brazo"],
    ["Headboard Height", "Altura da cabeceira", "Altura del cabecero"],
    ["Worktop", "Bancada", "Encimera"],
    // What drawings print.
    ["UP", "SOBE", "SUBE"],
    ["DN", "DESCE", "BAJA"],
    ["NOT ENCLOSED", "NÃO DELIMITADO", "NO DELIMITADA"],
    ["SCALE", "ESCALA", "ESCALA"],
    ["SCALE AS INDICATED", "ESCALA CONFORME INDICADO", "ESCALA SEGÚN SE INDICA"],
    // Schedule headings.
    ["Number", "Número", "Número"],
    ["Name", "Nome", "Nombre"],
    ["Mark", "Marca", "Marca"],
    ["Type", "Tipo", "Tipo"],
    ["Width", "Largura", "Ancho"],
    ["Height", "Altura", "Altura"],
    ["Sill", "Peitoril", "Antepecho"],
    ["Wall", "Parede", "Muro"],
    ["Length", "Comprimento", "Longitud"],
    ["Thickness", "Espessura", "Espesor"],
    ["Area", "Área", "Área"],
    ["Perimeter", "Perímetro", "Perímetro"],
    ["Volume", "Volume", "Volumen"],
    ["Gross Floor Area", "Área bruta de piso", "Superficie bruta de planta"],
    ["Count", "Contagem", "Recuento"],
    ["Family", "Família", "Familia"],
    ["Offset", "Deslocamento", "Desfase"],
    ["Slope", "Inclinação", "Pendiente"],
    ["Plan Area", "Área em planta", "Área en planta"],
    ["Risers", "Espelhos", "Contrahuellas"],
    ["Riser Height", "Altura do espelho", "Altura de contrahuella"],
    ["Tread", "Piso", "Huella"],
    ["Total", "Total", "Total"],
    ["Yes", "Sim", "Sí"],
    ["No", "Não", "No"],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_come_in_the_projects_language() {
        let pt = Language::Portuguese;
        assert_eq!([pt.word("Level"), pt.word("UP"), pt.word("SCALE AS INDICATED")], ["Nível", "SOBE", "ESCALA CONFORME INDICADO"]);
        assert_eq!(Language::English.word("Level"), "Level");
        assert_eq!(pt.word("Id"), "Id", "a word with no translation stays");
        assert_eq!(pt.find("tabela de ambientes", &["Room Schedule", "Door Schedule"]), Some("Room Schedule"));
        assert_eq!(pt.find("Door Schedule", &["Room Schedule", "Door Schedule"]), Some("Door Schedule"));
        let es = Language::Spanish;
        assert_eq!([es.word("Level"), es.word("UP"), es.word("Room Schedule")], ["Nivel", "SUBE", "Tabla de habitaciones"]);
        assert_eq!(es.find("TABLA DE PUERTAS", &["Room Schedule", "Door Schedule"]), Some("Door Schedule"));
        // Codes as commands write them, and locales as systems give them.
        assert_eq!(
            ["pt-BR", "pt_BR.UTF-8", "es", "es-419", "Español", "en-US", "en", "de", "C", ""].map(Language::from_code),
            [Some(pt), Some(pt), Some(es), Some(es), Some(es), Some(Language::English), Some(Language::English), None, None, None]
        );
    }
}
