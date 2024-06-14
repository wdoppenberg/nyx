/*
    Nyx, blazing fast astrodynamics
    Copyright (C) 2018-onwards Christopher Rabotin <christopher.rabotin@gmail.com>

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU Affero General Public License as published
    by the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU Affero General Public License for more details.

    You should have received a copy of the GNU Affero General Public License
    along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use super::RK;

/// `Dormand45` is a [Dormand-Prince integrator](https://en.wikipedia.org/wiki/Dormand%E2%80%93Prince_method).
pub struct Dormand45 {}

impl RK for Dormand45 {
    const ORDER: u8 = 5;
    const STAGES: usize = 7;
    const A_COEFFS: &'static [f64] = &[
        1.0 / 5.0,
        3.0 / 40.0,
        9.0 / 40.0,
        44.0 / 45.0,
        -56.0 / 15.0,
        32.0 / 9.0,
        19_372.0 / 6_561.0,
        -25_360.0 / 2_187.0,
        64_448.0 / 6_561.0,
        -212.0 / 729.0,
        9_017.0 / 3_168.0,
        -355.0 / 33.0,
        46_732.0 / 5247.0,
        49.0 / 176.0,
        -5_103.0 / 18_656.0,
        35.0 / 384.0,
        0.0,
        500.0 / 1_113.0,
        125.0 / 192.0,
        -2_187.0 / 6_784.0,
        11.0 / 84.0,
    ];
    const B_COEFFS: &'static [f64] = &[
        35.0 / 384.0,
        0.0,
        500.0 / 1_113.0,
        125.0 / 192.0,
        -2_187.0 / 6_784.0,
        11.0 / 84.0,
        0.0,
        5_179.0 / 57_600.0,
        0.0,
        7_571.0 / 16_695.0,
        393.0 / 640.0,
        -92_097.0 / 339_200.0,
        187.0 / 2_100.0,
        1.0 / 40.0,
    ];
}

/// `Dormand78` is a [Dormand-Prince integrator](https://en.wikipedia.org/wiki/Dormand%E2%80%93Prince_method).
///
/// Coefficients taken from GMAT `src/base/propagator/PrinceDormand78.cpp`.
pub struct Dormand78 {}

impl RK for Dormand78 {
    const ORDER: u8 = 8;
    const STAGES: usize = 13;
    const A_COEFFS: &'static [f64] = &[
        1.0 / 18.0,
        1.0 / 48.0,
        1.0 / 16.0,
        1.0 / 32.0,
        0.0,
        3.0 / 32.0,
        5.0 / 16.0,
        0.0,
        -75.0 / 64.0,
        75.0 / 64.0,
        3.0 / 80.0,
        0.0,
        0.0,
        3.0 / 16.0,
        3.0 / 20.0,
        29_443_841.0 / 614_563_906.0,
        0.0,
        0.0,
        77_736_538.0 / 692_538_347.0,
        -28_693_883.0 / 1_125_000_000.0,
        23_124_283.0 / 1_800_000_000.0,
        16_016_141.0 / 946_692_911.0,
        0.0,
        0.0,
        61_564_180.0 / 158_732_637.0,
        22_789_713.0 / 633_445_777.0,
        545_815_736.0 / 2_771_057_229.0,
        -180_193_667.0 / 1_043_307_555.0,
        39_632_708.0 / 573_591_083.0,
        0.0,
        0.0,
        -433_636_366.0 / 683_701_615.0,
        -421_739_975.0 / 2_616_292_301.0,
        100_302_831.0 / 723_423_059.0,
        790_204_164.0 / 839_813_087.0,
        800_635_310.0 / 3_783_071_287.0,
        246_121_993.0 / 1_340_847_787.0,
        0.0,
        0.0,
        -37_695_042_795.0 / 15_268_766_246.0,
        -309_121_744.0 / 1_061_227_803.0,
        -12_992_083.0 / 490_766_935.0,
        6_005_943_493.0 / 2_108_947_869.0,
        393_006_217.0 / 1_396_673_457.0,
        123_872_331.0 / 1_001_029_789.0,
        -1_028_468_189.0 / 846_180_014.0,
        0.0,
        0.0,
        8_478_235_783.0 / 508_512_852.0,
        1_311_729_495.0 / 1_432_422_823.0,
        -10_304_129_995.0 / 1_701_304_382.0,
        -48_777_925_059.0 / 3_047_939_560.0,
        15_336_726_248.0 / 1_032_824_649.0,
        -45_442_868_181.0 / 3_398_467_696.0,
        3_065_993_473.0 / 597_172_653.0,
        185_892_177.0 / 718_116_043.0,
        0.0,
        0.0,
        -3_185_094_517.0 / 667_107_341.0,
        -477_755_414.0 / 1_098_053_517.0,
        -703_635_378.0 / 230_739_211.0,
        5_731_566_787.0 / 1_027_545_527.0,
        5_232_866_602.0 / 850_066_563.0,
        -4_093_664_535.0 / 808_688_257.0,
        3_962_137_247.0 / 1_805_957_418.0,
        65_686_358.0 / 487_910_083.0,
        403_863_854.0 / 491_063_109.0,
        0.0,
        0.0,
        -5_068_492_393.0 / 434_740_067.0,
        -411_421_997.0 / 543_043_805.0,
        652_783_627.0 / 914_296_604.0,
        11_173_962_825.0 / 925_320_556.0,
        -13_158_990_841.0 / 6_184_727_034.0,
        3_936_647_629.0 / 1_978_049_680.0,
        -160_528_059.0 / 685_178_525.0,
        248_638_103.0 / 1_413_531_060.0,
        0.0,
    ];
    const B_COEFFS: &'static [f64] = &[
        14_005_451.0 / 335_480_064.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -59_238_493.0 / 1_068_277_825.0,
        181_606_767.0 / 758_867_731.0,
        561_292_985.0 / 797_845_732.0,
        -1_041_891_430.0 / 1_371_343_529.0,
        760_417_239.0 / 1_151_165_299.0,
        118_820_643.0 / 751_138_087.0,
        -528_747_749.0 / 2_220_607_170.0,
        0.25,
        13_451_932.0 / 455_176_623.0,
        0.0,
        0.0,
        0.0,
        0.0,
        -808_719_846.0 / 976_000_145.0,
        1_757_004_468.0 / 5_645_159_321.0,
        656_045_339.0 / 265_891_186.0,
        -3_867_574_721.0 / 1_518_517_206.0,
        465_885_868.0 / 322_736_535.0,
        53_011_238.0 / 667_516_719.0,
        2.0 / 45.0,
        0.0,
    ];
}
