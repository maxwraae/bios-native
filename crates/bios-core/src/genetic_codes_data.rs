//! The NCBI genetic code tables. Ported from the Bios web app, which generates them from NCBI's
//! `gc.prt`, version 4.6 (https://ftp.ncbi.nlm.nih.gov/entrez/misc/data/gc.prt, SHA-256
//! 2aecbdd09ecc5e0d233c105090a593737298ef93d88f6bc0c1bef26df6051a04).
//!
//! Each table holds one character per codon, the codons in T, C, A, G order at each position.

use crate::coding::GeneticCode;

pub static GENETIC_CODES: [GeneticCode; 27] = [
    GeneticCode {
        id: 1,
        name: "Standard",
        amino_acids: "FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "---M------**--*----M---------------M----------------------------",
    },
    GeneticCode {
        id: 2,
        name: "Vertebrate Mitochondrial",
        amino_acids: "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSS**VVVVAAAADDEEGGGG",
        start_stops: "----------**--------------------MMMM----------**---M------------",
    },
    GeneticCode {
        id: 3,
        name: "Yeast Mitochondrial",
        amino_acids: "FFLLSSSSYY**CCWWTTTTPPPPHHQQRRRRIIMMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "----------**----------------------MM---------------M------------",
    },
    GeneticCode {
        id: 4,
        name: "Mold Mitochondrial; Protozoan Mitochondrial; Coelenterate\n Mitochondrial; Mycoplasma; Spiroplasma",
        amino_acids: "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "--MM------**-------M------------MMMM---------------M------------",
    },
    GeneticCode {
        id: 5,
        name: "Invertebrate Mitochondrial",
        amino_acids: "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSSSSVVVVAAAADDEEGGGG",
        start_stops: "---M------**--------------------MMMM---------------M------------",
    },
    GeneticCode {
        id: 6,
        name: "Ciliate Nuclear; Dasycladacean Nuclear; Hexamita Nuclear",
        amino_acids: "FFLLSSSSYYQQCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "--------------*--------------------M----------------------------",
    },
    GeneticCode {
        id: 9,
        name: "Echinoderm Mitochondrial; Flatworm Mitochondrial",
        amino_acids: "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNNKSSSSVVVVAAAADDEEGGGG",
        start_stops: "----------**-----------------------M---------------M------------",
    },
    GeneticCode {
        id: 10,
        name: "Euplotid Nuclear",
        amino_acids: "FFLLSSSSYY**CCCWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "----------**-----------------------M----------------------------",
    },
    GeneticCode {
        id: 11,
        name: "Bacterial, Archaeal and Plant Plastid",
        amino_acids: "FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "---M------**--*----M------------MMMM---------------M------------",
    },
    GeneticCode {
        id: 12,
        name: "Alternative Yeast Nuclear",
        amino_acids: "FFLLSSSSYY**CC*WLLLSPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "----------**--*----M---------------M----------------------------",
    },
    GeneticCode {
        id: 13,
        name: "Ascidian Mitochondrial",
        amino_acids: "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNKKSSGGVVVVAAAADDEEGGGG",
        start_stops: "---M------**----------------------MM---------------M------------",
    },
    GeneticCode {
        id: 14,
        name: "Alternative Flatworm Mitochondrial",
        amino_acids: "FFLLSSSSYYY*CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNNKSSSSVVVVAAAADDEEGGGG",
        start_stops: "-----------*-----------------------M----------------------------",
    },
    GeneticCode {
        id: 15,
        name: "Blepharisma Macronuclear",
        amino_acids: "FFLLSSSSYY*QCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "----------*---*--------------------M----------------------------",
    },
    GeneticCode {
        id: 16,
        name: "Chlorophycean Mitochondrial",
        amino_acids: "FFLLSSSSYY*LCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "----------*---*--------------------M----------------------------",
    },
    GeneticCode {
        id: 21,
        name: "Trematode Mitochondrial",
        amino_acids: "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIMMTTTTNNNKSSSSVVVVAAAADDEEGGGG",
        start_stops: "----------**-----------------------M---------------M------------",
    },
    GeneticCode {
        id: 22,
        name: "Scenedesmus obliquus Mitochondrial",
        amino_acids: "FFLLSS*SYY*LCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "------*---*---*--------------------M----------------------------",
    },
    GeneticCode {
        id: 23,
        name: "Thraustochytrium Mitochondrial",
        amino_acids: "FF*LSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "--*-------**--*-----------------M--M---------------M------------",
    },
    GeneticCode {
        id: 24,
        name: "Rhabdopleuridae Mitochondrial",
        amino_acids: "FFLLSSSSYY**CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSSKVVVVAAAADDEEGGGG",
        start_stops: "---M------**-------M---------------M---------------M------------",
    },
    GeneticCode {
        id: 25,
        name: "Candidate Division SR1 and Gracilibacteria",
        amino_acids: "FFLLSSSSYY**CCGWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "---M------**-----------------------M---------------M------------",
    },
    GeneticCode {
        id: 26,
        name: "Pachysolen tannophilus Nuclear",
        amino_acids: "FFLLSSSSYY**CC*WLLLAPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "----------**--*----M---------------M----------------------------",
    },
    GeneticCode {
        id: 27,
        name: "Karyorelict Nuclear",
        amino_acids: "FFLLSSSSYYQQCCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "--------------*--------------------M----------------------------",
    },
    GeneticCode {
        id: 28,
        name: "Condylostoma Nuclear",
        amino_acids: "FFLLSSSSYYQQCCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "----------**--*--------------------M----------------------------",
    },
    GeneticCode {
        id: 29,
        name: "Mesodinium Nuclear",
        amino_acids: "FFLLSSSSYYYYCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "--------------*--------------------M----------------------------",
    },
    GeneticCode {
        id: 30,
        name: "Peritrich Nuclear",
        amino_acids: "FFLLSSSSYYEECC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "--------------*--------------------M----------------------------",
    },
    GeneticCode {
        id: 31,
        name: "Blastocrithidia Nuclear",
        amino_acids: "FFLLSSSSYYEECCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "----------**-----------------------M----------------------------",
    },
    GeneticCode {
        id: 32,
        name: "Balanophoraceae Plastid",
        amino_acids: "FFLLSSSSYY*WCC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG",
        start_stops: "---M------*---*----M------------MMMM---------------M------------",
    },
    GeneticCode {
        id: 33,
        name: "Cephalodiscidae Mitochondrial",
        amino_acids: "FFLLSSSSYYY*CCWWLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSSKVVVVAAAADDEEGGGG",
        start_stops: "---M-------*-------M---------------M---------------M------------",
    },
];
