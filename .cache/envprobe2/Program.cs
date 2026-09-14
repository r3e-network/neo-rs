using System;
class P {
  static void Main() {
    foreach (var f in new[]{Environment.SpecialFolder.CommonApplicationData, Environment.SpecialFolder.ApplicationData, Environment.SpecialFolder.LocalApplicationData}) {
      try { Console.WriteLine($"{f} = [{Environment.GetFolderPath(f)}]"); }
      catch (Exception e) { Console.WriteLine($"{f} => EX {e.GetType().Name}: {e.Message}"); }
    }
    Console.WriteLine("env ProgramData = [" + Environment.GetEnvironmentVariable("ProgramData") + "]");
  }
}
