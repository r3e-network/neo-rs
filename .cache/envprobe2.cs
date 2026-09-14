
using System;
class P{ static void Main(){
 Console.WriteLine("CommonApplicationData=["+Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData)+"]");
 Console.WriteLine("ApplicationData=["+Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData)+"]");
 Console.WriteLine("env ProgramData=["+Environment.GetEnvironmentVariable("ProgramData")+"]");
}}
