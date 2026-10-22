//! Ein kleines, eigenständiges Programm: es fragt interaktiv nach einer Anzahl und gibt so viele
//! Fibonacci-Zahlen aus, jede rekursiv berechnet.
//!
//! Bauen und starten (nichts außer `rustc` nötig):
//!
//! ```sh
//! rustc fibonacci.rs && ./fibonacci
//! ```
//!
//! Absichtlich ohne Cargo und ohne Abhängigkeiten: eine Datei, die den rekursiven Kern allein zeigt.

use std::io::{self, Write};

/// Berechnet die `n`-te Fibonacci-Zahl **rekursiv** (F(0) = 0, F(1) = 1).
///
/// Die Definition folgt direkt der mathematischen: `F(n) = F(n-1) + F(n-2)`, mit `F(0) = 0`
/// und `F(1) = 1` als Abbruch der Rekursion. Jeder Aufruf verzweigt sich also in zwei weitere,
/// bis er auf einem der beiden Grundfälle landet.
///
/// # Argumente
///
/// * `n` – der Index (nullbasiert); die Anzahl der Aufrufe hängt vom Wert ab, nicht von der
///   Anzahl der Ausgaben.
///
/// # Rückgabe
///
/// Die `n`-te Fibonacci-Zahl als `u128` – die Grundfälle 0 und 1 eingeschlossen.
///
/// # Beispiele
///
/// ```
/// assert_eq!(fibonacci(0), 0);
/// assert_eq!(fibonacci(1), 1);
/// assert_eq!(fibonacci(10), 55);
/// assert_eq!(fibonacci(20), 6765);
/// ```
///
/// # Komplexität
///
/// Naiv rekursiv: ohne Zwischenspeicher ist die Laufzeit exponentiell (etwa `O(1.618^n)`), weil
/// dasselbe Teilergebnis immer wieder neu berechnet wird – schon `fibonacci(40)` braucht
/// spürbar lange. Für große `n` wäre eine iterative Fassung oder Memoisierung richtig; hier ist
/// die Rekursion der Zweck.
///
/// # Überlauf
///
/// `u128` reicht bis `fibonacci(186)`; darüber läuft die Addition über. In einem Debug-Build
/// panikt Rust dann mit `attempt to add with overflow`, im Release-Build wickelt es still um.
fn fibonacci(n: u64) -> u128 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

/// Höchster Index, den [`fibonacci`] in `u128` noch ohne Überlauf darstellen kann (F(186)).
const MAX_N: u64 = 186;

/// Fragt den Nutzer interaktiv nach einer Anzahl und liest die Antwort von der Standardeingabe.
///
/// Die Frage wird ausgegeben und sofort geleert, damit sie auch ohne Zeilenumbruch erscheint.
/// Wiederholt, bis eine gültige Zahl eingegeben wurde; bei Dateiende (`Ctrl-D`) oder einem
/// Lesefehler gibt die Funktion `None` zurück.
///
/// # Argumente
///
/// * `prompt` – der Text der Frage, z. B. `"Wie viele Fibonacci-Zahlen? "`.
/// * `max` – Obergrenze; größere Eingaben werden mit einem Hinweis abgelehnt.
fn read_count(prompt: &str, max: u64) -> Option<u64> {
    loop {
        print!("{prompt}");
        io::stdout().flush().ok()?;

        let mut line = String::new();
        if io::stdin().read_line(&mut line).ok()? == 0 {
            println!(); // Nutzer hat abgebrochen (Strg-D): sauber auf eine neue Zeile.
            return None;
        }

        match line.trim().parse::<u64>() {
            Ok(n) if n <= max => return Some(n),
            Ok(n) => println!("  {n} ist zu groß – höchstens {max} (sonst läuft u128 über)."),
            Err(_) => println!("  Bitte eine ganze, nicht-negative Zahl eingeben."),
        }
    }
}

fn main() {
    println!("Fibonacci – rekursiv berechnet");

    let Some(count) = read_count(
        &format!("Wie viele Fibonacci-Zahlen (0 bis {MAX_N})? "),
        MAX_N,
    ) else {
        println!("Keine Eingabe – Programm beendet.");
        return;
    };

    println!("\nDie ersten {count} Fibonacci-Zahlen:");
    for n in 0..count {
        println!("  F({n}) = {}", fibonacci(n));
    }
}
