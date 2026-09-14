// Environment probe: emits what a child .NET process actually sees.
Console.WriteLine($"PROGRAMDATA=[{Environment.GetEnvironmentVariable("PROGRAMDATA")}]");
Console.WriteLine($"ProgramData=[{Environment.GetEnvironmentVariable("ProgramData")}]");
Console.WriteLine($"APPDATA=[{Environment.GetEnvironmentVariable("APPDATA")}]");
Console.WriteLine($"NuGetMachineWide=[{Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData)}]");
