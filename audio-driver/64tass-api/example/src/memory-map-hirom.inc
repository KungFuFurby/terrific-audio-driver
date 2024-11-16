; HIROM memory map

; SPDX-FileCopyrightText: © 2024 Marcus Rowe <undisbeliever@gmail.com>
; SPDX-License-Identifier: Zlib
;
; Copyright © 2024 Marcus Rowe <undisbeliever@gmail.com>
;
; This software is provided 'as-is', without any express or implied warranty.  In
; no event will the authors be held liable for any damages arising from the use of
; this software.
;
; Permission is granted to anyone to use this software for any purpose, including
; commercial applications, and to alter it and redistribute it freely, subject to
; the following restrictions:
;
;      1. The origin of this software must not be misrepresented; you must not
;         claim that you wrote the original software. If you use this software in
;         a product, an acknowledgment in the product documentation would be
;         appreciated but is not required.
;
;      2. Altered source versions must be plainly marked as such, and must not be
;         misrepresented as being the original software.
;
;      3. This notice may not be removed or altered from any source distribution.


; 2Mbits = 256KiB
ROM_SIZE = 2

* = $000000             ; ROM offset
.logical $c00000        ; start address
    .dsection BankC0
        .cerror * > $c18000, "Bank c0 overflow by ", * - $c08000
    .if * == $c00000
        ; Required
        .byte 0
    .endif
.here

* = $008000             ; ROM offset
.logical $808000        ; start address
    .dsection Code
        .cerror * > $80ffb0, "Bank 80 overflow by ", * - $80ffb0
.here

* = $00ffb0
.logical $80ffb0
    .dsection SnesHeader
        .cerror * != $810000, "Missing header"
.here

* = $010000             ; ROM offset
.logical $c10000        ; start address
    .dsection BankC1
        .cerror * > $c20000, "Bank c1 overflow by ", * - $c20000
.here

* = $020000             ; ROM offset
.logical $c20000        ; start address
    .dsection BankC2
        .cerror * > $c30000, "Bank c2 overflow by ", * - $c30000
.here

* = $030000             ; ROM offset
.logical $c30000        ; start address
    .dsection BankC3
        .cerror * > $c40000, "Bank c3 overflow by ", * - $c40000
.here

; Padding
* = $03ffff
    .byte 0


; RAM map
START_TOP    = $1fff
STACK_BOTTOM = $1f80

* = $0000
.dsection Zeropage
    .cerror * > $100, "zeropage overflow"

* = $000100
.dsection Lowram
    .cerror * > STACK_BOTTOM, "lowram section overflow"

* = $7e2000
.dsection Wram7e
    .cerror * > $7effff, "wram7e section overflow"

* = $7f0000
.dsection Wram7f
    .cerror * > $7fffff, "wram7f section overflow"


* = 0
