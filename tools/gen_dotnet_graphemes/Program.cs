// Generates the .NET grapheme-cluster break-property table and the strLen
// oracle fixture used by neo-native-contracts (StdLib.strLen parity).
//
// Neo's C# StdLib.StrLen counts .NET text elements (StringInfo /
// TextElementEnumerator). .NET implements UAX #29 extended grapheme clusters
// WITHOUT rule GB9c (Indic conjunct breaks) and with the Unicode data snapshot
// bundled into System.Private.CoreLib, so third-party UAX #29 libraries (which
// track newer Unicode versions and do implement GB9c) cannot be used as a
// stand-in. This program derives the break-property class of every code point
// from the installed .NET runtime itself:
//
//   1. behavioural probing: each code point is classified by measuring
//      StringInfo text-element counts of fingerprint strings whose counts
//      uniquely identify the break class under .NET's segmentation;
//   2. cross-check: the internal CharUnicodeInfo.GetGraphemeClusterBreakType
//      table is read via reflection and must agree on every code point;
//   3. machine check: a mirror of the Rust segmentation state machine is run
//      over the generated table for every fixture string and must reproduce
//      StringInfo's text-element count exactly.
//
// Outputs (written to the directory given as the first argument):
//   dotnet_graphemes.rs        range-run break-property table (start, end, class)
//   dotnet_strlen_oracle.txt   fixture: "count<TAB>hex hex ..." per line
//
// Usage: dotnet run -c Release -- <out_dir>
//
// All fixture content is spelled with explicit \uXXXX / \UXXXXXXXX escapes so
// the oracle cannot be corrupted by editor normalization.

using System.Globalization;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;

internal static class Program
{
    private const int CodePointCount = 0x110000;

    private const byte ClsOther = 0;
    private const byte ClsCR = 1;
    private const byte ClsLF = 2;
    private const byte ClsControl = 3;
    private const byte ClsExtend = 4;
    private const byte ClsZWJ = 5;
    private const byte ClsRI = 6;
    private const byte ClsPrepend = 7;
    private const byte ClsSpacingMark = 8;
    private const byte ClsL = 9;
    private const byte ClsV = 10;
    private const byte ClsT = 11;
    private const byte ClsLV = 12;
    private const byte ClsLVT = 13;
    private const byte ClsExtPict = 14;

    private static readonly string[] ClassNames =
    {
        "Other", "CR", "LF", "Control", "Extend", "ZWJ", "RegionalIndicator", "Prepend",
        "SpacingMark", "HangulL", "HangulV", "HangulT", "HangulLV", "HangulLVT",
        "ExtendedPictographic",
    };

    private static int Main(string[] args)
    {
        string outDir = args.Length > 0 ? args[0] : ".";
        Directory.CreateDirectory(outDir);

        Console.WriteLine($"runtime: {RuntimeInformation.FrameworkDescription}");

        // ---- Pin the C# UT_StdLib vectors and the GB9c divergence up front. ----
        Check(Len("") == 0, "empty string must be 0 text elements");
        Check(Len("a") == 1, "'a' must be 1 text element");
        Check(Len("\U0001F986") == 1, "duck emoji must be 1 text element");
        Check(Len("ã") == 1, "precomposed a-tilde must be 1 text element");
        Check(Len("ã") == 1, "decomposed a-tilde must be 1 text element");
        Check(Len("ÿ") == 1, "U+00FF must be 1 text element");
        Check(Len("ÿab") == 3, "U+00FF + \"ab\" must be 3 text elements");
        Check(Len("क्क") == 2,
            ".NET must NOT apply GB9c: Devanagari KA+virama+KA is 2 text elements");

        // ---- Classify every code point by behavioural probing. ----
        byte[] probed = new byte[CodePointCount];
        Parallel.For(0, CodePointCount, cp =>
        {
            if (cp is >= 0xD800 and <= 0xDFFF)
            {
                probed[cp] = ClsOther; // surrogates: unreachable from valid UTF-8
                return;
            }
            probed[cp] = Classify(cp);
        });
        Console.WriteLine("probing: classified all code points");

        // Sanity-pin canonical representatives of each class.
        CheckClass(probed, 0x0041, ClsOther);
        CheckClass(probed, 0x000D, ClsCR);
        CheckClass(probed, 0x000A, ClsLF);
        CheckClass(probed, 0x0001, ClsControl);
        CheckClass(probed, 0x200B, ClsControl); // ZERO WIDTH SPACE is Control
        CheckClass(probed, 0x0300, ClsExtend);
        CheckClass(probed, 0x200C, ClsExtend); // ZWNJ is Extend
        CheckClass(probed, 0x200D, ClsZWJ);
        CheckClass(probed, 0x1F1E6, ClsRI);
        CheckClass(probed, 0x0600, ClsPrepend);
        CheckClass(probed, 0x0903, ClsSpacingMark);
        CheckClass(probed, 0x1100, ClsL);
        CheckClass(probed, 0x1160, ClsV);
        CheckClass(probed, 0x11A8, ClsT);
        CheckClass(probed, 0xAC00, ClsLV);
        CheckClass(probed, 0xAC01, ClsLVT);
        CheckClass(probed, 0x1F600, ClsExtPict);
        CheckClass(probed, 0x00A9, ClsExtPict); // COPYRIGHT SIGN is Extended_Pictographic
        CheckClass(probed, 0x094D, ClsExtend); // Devanagari virama
        CheckClass(probed, 0x0915, ClsOther); // Devanagari KA

        // ---- Cross-check against .NET's internal break-property table. ----
        bool crossChecked = CrossCheckWithReflection(probed);

        // ---- Build pools per class for fixture generation. ----
        var pools = new List<int>[15];
        for (int i = 0; i < 15; i++) pools[i] = new List<int>();
        for (int cp = 0; cp < CodePointCount; cp++)
        {
            if (cp is >= 0xD800 and <= 0xDFFF) continue;
            byte cls = probed[cp];
            if (cls == ClsOther)
            {
                // The Other pool is huge; stride-sample it but keep printable ASCII.
                if (cp % 257 == 0 || (cp is >= 0x20 and <= 0x7E)) pools[cls].Add(cp);
            }
            else
            {
                pools[cls].Add(cp);
            }
        }
        for (int i = 0; i < 15; i++)
        {
            Check(pools[i].Count > 0, $"class pool {ClassNames[i]} must not be empty");
            Console.WriteLine($"pool {ClassNames[i],-22} {pools[i].Count}");
        }

        // ---- Generate the fixture strings. ----
        var rnd = new Random(48271);
        var fixtures = new List<string>();

        fixtures.AddRange(CuratedCases());

        // Systematic ordered class pairs (3 random instantiations each).
        for (int c1 = 0; c1 < 15; c1++)
            for (int c2 = 0; c2 < 15; c2++)
                for (int k = 0; k < 3; k++)
                    fixtures.Add(Cp(Pick(pools[c1], rnd)) + Cp(Pick(pools[c2], rnd)));

        // Systematic sandwich triples around segmentation-critical middles:
        // ZWJ (GB11), a combining mark (GB9), a virama (GB9c divergence), and a
        // spacing mark (GB9a / GB11-chain breaking).
        int[] middles = { 0x200D, 0x0300, 0x094D, 0x0903 };
        foreach (int mid in middles)
            for (int c1 = 0; c1 < 15; c1++)
                for (int c2 = 0; c2 < 15; c2++)
                    fixtures.Add(Cp(Pick(pools[c1], rnd)) + Cp(mid) + Cp(Pick(pools[c2], rnd)));

        // Regional-indicator runs (GB12/GB13 parity) with varied context.
        for (int n = 1; n <= 8; n++)
        {
            string run = string.Concat(Enumerable.Repeat("\U0001F1E6", n));
            fixtures.Add(run);
            fixtures.Add("x" + run);
            fixtures.Add(run + "x");
            fixtures.Add("؀" + run); // Prepend + RI run
            fixtures.Add(run + "̀"); // RI run + Extend
        }

        // Prepend chains and control interactions.
        fixtures.Add("؀؀؀");
        fixtures.Add("؀؀a");
        fixtures.Add("؀\r");
        fixtures.Add("؀\n");
        fixtures.Add("؀\r\n");
        fixtures.Add("؀‍");
        fixtures.Add("؀\U0001F600‍\U0001F600");
        fixtures.Add("a\r\nb\r\n");
        fixtures.Add("\r\r\n\n");
        fixtures.Add("\r̀");
        fixtures.Add("\ǹ");
        fixtures.Add("​̀");

        // Random multi-script strings: scalars drawn from the class pools (55%),
        // printable ASCII (20%), random BMP (15%), random astral (10%).
        while (fixtures.Count < 5600)
        {
            int len = 1 + rnd.Next(10);
            var sb = new StringBuilder();
            for (int i = 0; i < len; i++)
            {
                int roll = rnd.Next(100);
                int cp;
                if (roll < 55)
                {
                    cp = Pick(pools[rnd.Next(15)], rnd);
                }
                else if (roll < 75)
                {
                    cp = 0x20 + rnd.Next(0x5F);
                }
                else if (roll < 90)
                {
                    do { cp = rnd.Next(0x10000); } while (cp is >= 0xD800 and <= 0xDFFF);
                }
                else
                {
                    cp = 0x10000 + rnd.Next(0x100000);
                }
                sb.Append(Cp(cp));
            }
            fixtures.Add(sb.ToString());
        }

        Console.WriteLine($"fixtures: {fixtures.Count} strings");

        // ---- Validate the state-machine mirror against StringInfo on each. ----
        int machineMismatches = 0;
        foreach (string s in fixtures)
        {
            int expected = Len(s);
            int got = MachineCount(s, probed);
            if (expected != got)
            {
                machineMismatches++;
                if (machineMismatches <= 10)
                {
                    Console.WriteLine(
                        $"MACHINE MISMATCH: [{HexScalars(s)}] StringInfo={expected} machine={got}");
                }
            }
        }
        Check(machineMismatches == 0,
            $"segmentation machine mirror diverged from StringInfo on {machineMismatches} strings");
        Console.WriteLine("machine mirror: matches StringInfo on every fixture string");

        // ---- Emit the fixture file. ----
        var fix = new StringBuilder();
        fix.Append("# StdLib.strLen oracle fixture. Generated by tools/gen_dotnet_graphemes.\n");
        fix.Append($"# Oracle: StringInfo text-element counts from {RuntimeInformation.FrameworkDescription}.\n");
        fix.Append("# Format: <count>\\t<space-separated scalar hex values> (no scalars for the empty string).\n");
        foreach (string s in fixtures)
        {
            fix.Append(Len(s).ToString(CultureInfo.InvariantCulture));
            fix.Append('\t');
            fix.Append(HexScalars(s));
            fix.Append('\n');
        }
        File.WriteAllText(Path.Combine(outDir, "dotnet_strlen_oracle.txt"), fix.ToString());

        // ---- Emit the range-run table as Rust source. ----
        var runs = new List<(int Start, int End, byte Cls)>();
        int runStart = 0;
        byte runCls = probed[0];
        for (int cp = 1; cp < CodePointCount; cp++)
        {
            if (probed[cp] != runCls)
            {
                if (runCls != ClsOther) runs.Add((runStart, cp - 1, runCls));
                runStart = cp;
                runCls = probed[cp];
            }
        }
        if (runCls != ClsOther) runs.Add((runStart, CodePointCount - 1, runCls));
        Console.WriteLine($"table: {runs.Count} non-Other ranges");

        var rs = new StringBuilder();
        rs.Append("//! .NET grapheme-cluster break-property table (`StdLib.strLen` parity).\n");
        rs.Append("//!\n");
        rs.Append("//! GENERATED FILE - do not edit by hand. Regenerate with:\n");
        rs.Append("//!\n");
        rs.Append("//! ```text\n");
        rs.Append("//! dotnet run -c Release --project tools/gen_dotnet_graphemes -- <out_dir>\n");
        rs.Append("//! ```\n");
        rs.Append("//!\n");
        rs.Append("//! Source: per-code-point behavioural probing of `StringInfo` on\n");
        rs.Append($"//! {RuntimeInformation.FrameworkDescription}");
        rs.Append(crossChecked
            ? ", cross-checked code point by code point\n//! against the runtime's internal `CharUnicodeInfo.GetGraphemeClusterBreakType`\n//! table via reflection (zero disagreements).\n"
            : ".\n");
        rs.Append("//!\n");
        rs.Append("//! Each entry is `(first_code_point, last_code_point, class)` for the\n");
        rs.Append("//! non-`Other` ranges; every code point not covered is class `Other` (0).\n");
        rs.Append("//! Class ids match `GraphemeBreakClass` in `dotnet_text_segmentation.rs`:\n");
        rs.Append("//!\n");
        for (int i = 0; i < 15; i++) rs.Append($"//! - {i} = {ClassNames[i]}\n");
        rs.Append("\n");
        rs.Append("/// Sorted, non-overlapping `(start, end, class)` runs of the non-`Other`\n");
        rs.Append("/// grapheme-cluster break classes, as probed from the .NET runtime.\n");
        rs.Append("#[rustfmt::skip]\n");
        rs.Append("pub(crate) static DOTNET_GRAPHEME_BREAK_RANGES: &[(u32, u32, u8)] = &[\n");
        for (int i = 0; i < runs.Count; i++)
        {
            if (i % 6 == 0) rs.Append("    ");
            var (s, e, c) = runs[i];
            rs.Append($"(0x{s:X}, 0x{e:X}, {c}),");
            rs.Append((i % 6 == 5 || i == runs.Count - 1) ? "\n" : " ");
        }
        rs.Append("];\n");
        File.WriteAllText(Path.Combine(outDir, "dotnet_graphemes.rs"), rs.ToString());

        Console.WriteLine($"wrote {Path.Combine(outDir, "dotnet_graphemes.rs")}");
        Console.WriteLine($"wrote {Path.Combine(outDir, "dotnet_strlen_oracle.txt")}");
        Console.WriteLine("OK");
        return 0;
    }

    /// <summary>Text-element count, exactly as C# StdLib.StrLen computes it.</summary>
    private static int Len(string s)
    {
        TextElementEnumerator enumerator = StringInfo.GetTextElementEnumerator(s);
        int count = 0;
        while (enumerator.MoveNext()) count++;
        return count;
    }

    private static string Cp(int cp) => char.ConvertFromUtf32(cp);

    private static int Pick(List<int> pool, Random rnd) => pool[rnd.Next(pool.Count)];

    private static string HexScalars(string s)
    {
        var parts = new List<string>();
        foreach (System.Text.Rune r in s.EnumerateRunes())
            parts.Add(r.Value.ToString("X", CultureInfo.InvariantCulture));
        return string.Join(' ', parts);
    }

    private static void Check(bool condition, string message)
    {
        if (!condition)
        {
            Console.Error.WriteLine($"FATAL: {message}");
            Environment.Exit(1);
        }
    }

    private static void CheckClass(byte[] table, int cp, byte expected)
    {
        Check(table[cp] == expected,
            $"U+{cp:X4} probed as {ClassNames[table[cp]]}, expected {ClassNames[expected]}");
    }

    /// <summary>
    /// Classifies a code point into its grapheme-cluster break class purely from
    /// the observable behaviour of StringInfo. Each branch condition is a
    /// fingerprint: under .NET's segmentation (UAX #29 extended grapheme
    /// clusters without GB9c) the text-element count of the probe string is 1
    /// exactly for the classes noted.
    /// </summary>
    private static byte Classify(int cp)
    {
        string u = Cp(cp);
        const string A = "A";
        const string Ext = "̀"; // COMBINING GRAVE ACCENT (Extend)
        const string Zwj = "‍"; // ZERO WIDTH JOINER
        const string Ri = "\U0001F1E6"; // REGIONAL INDICATOR SYMBOL LETTER A
        const string Ep = "\U0001F600"; // GRINNING FACE (Extended_Pictographic)
        const string Hl = "ᄀ"; // HANGUL CHOSEONG KIYEOK (L)
        const string Hv = "ᅠ"; // HANGUL JUNGSEONG FILLER (V)
        const string Ht = "ᆨ"; // HANGUL JONGSEONG KIYEOK (T)

        // [U, Extend] merges (GB9) for every class except {CR, LF, Control} (GB4).
        if (Len(u + Ext) == 2)
        {
            if (Len(u + "\n") == 1) return ClsCR; // GB3: only CR x LF
            if (Len("\r" + u) == 1) return ClsLF; // GB3 from the other side
            return ClsControl;
        }
        // [A, U] merges (GB9/GB9a) only for {Extend, ZWJ, SpacingMark}.
        if (Len(A + u) == 1)
        {
            // [EP, U, EP] is one element only for U=ZWJ (GB11).
            if (Len(Ep + u + Ep) == 1) return ClsZWJ;
            // [EP, U, ZWJ, EP] stays one element only if U=Extend keeps the
            // GB11 chain alive; a SpacingMark in the chain forces a break.
            if (Len(Ep + u + Zwj + Ep) == 1) return ClsExtend;
            return ClsSpacingMark;
        }
        // [U, A] merges only for U=Prepend (GB9b).
        if (Len(u + A) == 1) return ClsPrepend;
        // [U, RI] merges only for U=RI (GB12; Prepend was peeled above).
        if (Len(u + Ri) == 1) return ClsRI;
        // [EP, ZWJ, U] merges only for U=Extended_Pictographic (GB11; the
        // Extend/ZWJ/SpacingMark cases were peeled above).
        if (Len(Ep + Zwj + u) == 1) return ClsExtPict;
        // [U, V] merges for U in {L, V, LV} (GB6/GB7).
        if (Len(u + Hv) == 1)
        {
            if (Len(u + Hl) == 1) return ClsL; // GB6: L x L
            if (Len(Hv + u) == 1) return ClsV; // GB7: V x V
            return ClsLV;
        }
        // [U, T] merges for U in {T, LVT} here (V/LV/Prepend peeled above; GB7/GB8).
        if (Len(u + Ht) == 1)
        {
            if (Len(Hl + u) == 1) return ClsLVT; // GB6: L x LVT
            return ClsT;
        }
        return ClsOther;
    }

    /// <summary>
    /// Reads .NET's internal per-code-point break-property table via reflection
    /// and verifies it agrees with the probed classification on every code
    /// point. Returns true when the cross-check ran and passed; aborts the
    /// process on any disagreement or unmappable enum member.
    /// </summary>
    private static bool CrossCheckWithReflection(byte[] probed)
    {
        MethodInfo? mi = typeof(CharUnicodeInfo).GetMethod(
            "GetGraphemeClusterBreakType",
            BindingFlags.NonPublic | BindingFlags.Static);
        if (mi is null || mi.GetParameters().Length != 1 ||
            mi.GetParameters()[0].ParameterType != typeof(System.Text.Rune))
        {
            Console.WriteLine(
                "reflection: CharUnicodeInfo.GetGraphemeClusterBreakType(Rune) not found; " +
                "skipping the internal-table cross-check");
            return false;
        }

        string[] enumNames = mi.ReturnType.IsEnum ? mi.ReturnType.GetEnumNames() : Array.Empty<string>();
        Console.WriteLine($"reflection: internal enum members = [{string.Join(", ", enumNames)}]");

        int mismatches = 0;
        object?[] argBuf = new object?[1];
        for (int cp = 0; cp < CodePointCount; cp++)
        {
            if (cp is >= 0xD800 and <= 0xDFFF) continue;
            argBuf[0] = new System.Text.Rune(cp);
            object result = mi.Invoke(null, argBuf)!;
            string name = result.ToString()!;
            byte mapped = MapInternalName(name);
            Check(mapped != byte.MaxValue,
                $"unmappable internal break-type enum member '{name}' at U+{cp:X4}");
            if (mapped != probed[cp])
            {
                mismatches++;
                if (mismatches <= 10)
                {
                    Console.WriteLine(
                        $"TABLE MISMATCH: U+{cp:X4} internal={name} probed={ClassNames[probed[cp]]}");
                }
            }
        }
        Check(mismatches == 0,
            $"probed table disagrees with the internal table on {mismatches} code points");
        Console.WriteLine("reflection: internal table agrees with probing on every code point");
        return true;
    }

    private static byte MapInternalName(string name) => name switch
    {
        "Other" => ClsOther,
        "CR" or "Cr" or "CarriageReturn" => ClsCR,
        "LF" or "Lf" or "LineFeed" => ClsLF,
        "Control" => ClsControl,
        "Extend" => ClsExtend,
        "ZWJ" or "Zwj" or "ZeroWidthJoiner" => ClsZWJ,
        "RegionalIndicator" or "Regional_Indicator" => ClsRI,
        "Prepend" => ClsPrepend,
        "SpacingMark" => ClsSpacingMark,
        "L" or "HangulL" => ClsL,
        "V" or "HangulV" => ClsV,
        "T" or "HangulT" => ClsT,
        "LV" or "HangulLV" => ClsLV,
        "LVT" or "HangulLVT" => ClsLVT,
        "ExtendedPictograph" or "ExtendedPictographic" or "Extended_Pictograph"
            or "Extended_Pictographic" => ClsExtPict,
        _ => byte.MaxValue,
    };

    /// <summary>
    /// Mirror of the Rust segmentation state machine in
    /// `neo-native-contracts/src/dotnet_text_segmentation.rs`: repeated
    /// first-cluster scanning over the break classes of the string's scalars.
    /// Must reproduce StringInfo's text-element count for every fixture string.
    /// </summary>
    private static int MachineCount(string s, byte[] table)
    {
        var classes = new List<byte>();
        foreach (System.Text.Rune r in s.EnumerateRunes()) classes.Add(table[r.Value]);
        int count = 0;
        int pos = 0;
        while (pos < classes.Count)
        {
            pos += FirstClusterLen(classes, pos);
            count++;
        }
        return count;
    }

    private static int FirstClusterLen(List<byte> classes, int start)
    {
        int n = classes.Count;
        int i = start;

        // GB9b: leading Prepend* attaches to what follows...
        if (classes[i] == ClsPrepend)
        {
            while (i < n && classes[i] == ClsPrepend) i++;
            if (i == n) return i - start; // GB2: only Prepend scalars left
            // ...unless a Control/CR/LF follows (GB5 outranks GB9b).
            if (classes[i] is ClsControl or ClsCR or ClsLF) return i - start;
        }

        byte state = classes[i];
        i++;

        // GB3/GB4: CR pairs only with LF; Control/LF terminate the cluster.
        if (state == ClsCR)
        {
            if (i < n && classes[i] == ClsLF) i++;
            return i - start;
        }
        if (state is ClsControl or ClsLF) return i - start;

        // GB6-GB8 (Hangul), GB11 (emoji ZWJ chains), GB12/GB13 (RI pairs).
        while (true)
        {
            if (state == ClsL)
            {
                if (i < n && classes[i] is ClsL or ClsV or ClsLV or ClsLVT)
                {
                    state = classes[i];
                    i++;
                    continue;
                }
            }
            else if (state is ClsLV or ClsV)
            {
                if (i < n && classes[i] is ClsV or ClsT)
                {
                    state = classes[i];
                    i++;
                    continue;
                }
            }
            else if (state is ClsLVT or ClsT)
            {
                if (i < n && classes[i] == ClsT)
                {
                    i++;
                    continue;
                }
            }
            else if (state == ClsExtPict)
            {
                while (i < n && classes[i] == ClsExtend) i++;
                if (i < n && classes[i] == ClsZWJ)
                {
                    if (i + 1 < n && classes[i + 1] == ClsExtPict)
                    {
                        i += 2; // GB11: EP Extend* ZWJ x EP, stay in the EP state
                        continue;
                    }
                    i++; // the ZWJ still attaches (GB9); the chain ends here
                }
            }
            else if (state == ClsRI)
            {
                if (i < n && classes[i] == ClsRI) i++; // GB12/GB13: pair up
            }
            break;
        }

        // GB9/GB9a: trailing Extend/ZWJ/SpacingMark always attach.
        while (i < n && classes[i] is ClsExtend or ClsZWJ or ClsSpacingMark) i++;
        return i - start;
    }

    private static List<string> CuratedCases() => new()
    {
        // C# UT_StdLib vectors.
        "", "a", "ã", "ã", "\U0001F986", "ÿ", "ÿab", "ab", "abc",
        // CR/LF.
        "\r\n", "\n\r", "\r", "\n", "a\r\nb", "\r\n\r\n", "a\nb\rc\r\nd", "\r\n\n\r\r\n",
        // Devanagari / Bengali / Khmer virama clusters (GB9c divergence pins).
        "क्क", "क्क्क",
        "नमस्ते", "क्‍क",
        "র্ম", "ন্তু", "ৰ্য",
        "ក្ក", "ម្តាយ",
        "ច្រើន",
        // Emoji ZWJ chains, modifiers, variation selectors.
        "\U0001F468‍\U0001F469‍\U0001F467‍\U0001F466",
        "\U0001F469‍❤️‍\U0001F48B‍\U0001F468",
        "\U0001F3F3️‍\U0001F308", "\U0001F3F3️‍⚧️",
        "\U0001F44D\U0001F3FD", "\U0001F926\U0001F3FC‍♂️",
        "\U0001F9D1\U0001F3FF‍\U0001F91D‍\U0001F9D1\U0001F3FB",
        "\U0001F431‍\U0001F409", "\U0001F408‍⬛",
        "\U0001F43B‍❄️", "\U0001F636‍\U0001F32B️",
        "❤️‍\U0001F525", "❤‍\U0001FA79",
        // Flags / regional indicators.
        "\U0001F1FA\U0001F1F8", "\U0001F1FA\U0001F1F8\U0001F1E9\U0001F1EA",
        "\U0001F1E6", "\U0001F1E6\U0001F1E7\U0001F1E8",
        "\U0001F1E6\U0001F1E7\U0001F1E8\U0001F1E9",
        "\U0001F1E6\U0001F1E7\U0001F1E8\U0001F1E9\U0001F1EA",
        // Tag sequence (flag of Scotland) and keycaps.
        "\U0001F3F4\U000E0067\U000E0062\U000E0073\U000E0063\U000E0074\U000E007F",
        "1️⃣", "#⃣", "*️⃣",
        // Extended_Pictographic oddballs: (c), (r), TM, mahjong, chess, snowman.
        "©", "©️", "™", "®‍\U0001F600", "©‍©",
        "\U0001F004", "\U0001F004‍\U0001F004", "♟", "♟️",
        "♟️‍♟️", "☃‍☃",
        // Hangul: precomposed, decomposed, jamo runs, boundary mixes.
        "가", "각", "가각", "가", "각",
        "ᄀ각ᆨ", "ᄀ가", "각", "각ᆨ",
        "ᅠᆨ", "ᆨᆨ", "ᅠᅠ", "ᆨᄀ", "각ᅡ",
        "가ᅡᆨ", "ힰퟋ", "ᄀ각ᆨ",
        // Prepend (Arabic number signs, Syriac abbreviation, Kaithi number sign).
        "؀١", "؀؀١", "؀", "۝١", "܏ܐ",
        "\U000110BD\U00011066", "\U000110CD\U00011066",
        // Format / control characters.
        "‍", "‌", "​", "﻿", "­", "\u2028", "\u2029", "\u2028a",
        "a‍b", "\U0001F600‍", "‍\U0001F600", "\U0001F600‍\U0001F600",
        "\U0001F600̀‍\U0001F600", "\U0001F600̀̀‍\U0001F600",
        "\U0001F600ः‍\U0001F600", "\U0001F600‍‍\U0001F600",
        // Stacked combining marks (Zalgo-style).
        "Z̴̡̟̀́ë́",
        "á̂̃̄̅̆̇̈̉",
        // Musical symbols (astral Extend) and other astral combining.
        "\U0001D161\U0001D165", "\U0001D16D", "a\U0001D165",
        "\U0001D158\U0001D165\U0001D16E",
        // Misc multi-script.
        "ཀཽ", "ༀཱི", "நி", "క్ష",
        "เด็ก", "ກ່", "ນຳ",
        "ﬃ", "ﷺ", "אַ", "اَل",
    };
}
