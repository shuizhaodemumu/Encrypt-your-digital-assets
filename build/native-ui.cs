using System;using System.Runtime.InteropServices;using System.Text;using System.Collections.Generic;
public static class SmokeWindows {
 [DllImport("user32.dll")]public static extern IntPtr GetParent(IntPtr h);
 [DllImport("user32.dll")]public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern IntPtr GetProp(IntPtr h,string name);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern bool SetProp(IntPtr h,string name,IntPtr value);
 [DllImport("user32.dll",EntryPoint="GetWindowLongPtrW")]public static extern IntPtr GetWindowLongPtr(IntPtr h,int index);
 public delegate bool Callback(IntPtr h,IntPtr p);
 [DllImport("user32.dll")]static extern bool EnumWindows(Callback cb,IntPtr p);
 [DllImport("user32.dll")]static extern bool EnumChildWindows(IntPtr h,Callback cb,IntPtr p);
 [DllImport("user32.dll")]static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern int GetWindowText(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern int GetClassName(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode,EntryPoint="SendMessageW")]static extern IntPtr GetTextMessage(IntPtr h,uint msg,IntPtr w,StringBuilder text);
 [DllImport("user32.dll")]public static extern int GetDlgCtrlID(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern IntPtr SendMessage(IntPtr h,uint msg,IntPtr w,string l);
 [DllImport("user32.dll")]public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 public static string Text(IntPtr h){var b=new StringBuilder(1000);if(Class(h).Contains("RICHEDIT")||Class(h)=="Edit")GetTextMessage(h,0xD,new IntPtr(1000),b);else GetWindowText(h,b,1000);return b.ToString();}
 public static string Class(IntPtr h){var b=new StringBuilder(300);GetClassName(h,b,300);return b.ToString();}
 public static IntPtr[] Children(IntPtr parent){var a=new List<IntPtr>();EnumChildWindows(parent,(h,p)=>{a.Add(h);return true;},IntPtr.Zero);return a.ToArray();}
 public static IntPtr[] ForProcess(int pid){var a=new List<IntPtr>();EnumWindows((h,p)=>{uint id;GetWindowThreadProcessId(h,out id);if(id==pid&&(Text(h).Length>0||Class(h)=="#32770"))a.Add(h);return true;},IntPtr.Zero);return a.ToArray();}
}
