package org.encryptassets.video;
import org.json.JSONObject;
public final class Core {
    static { System.loadLibrary("encrypt_video"); }
    public static native String call(String operation,String input);
    public static native byte[] read(long handle,long offset,int length) throws java.io.IOException;
    public static JSONObject invoke(String operation,JSONObject input) throws Exception {
        JSONObject r=new JSONObject(call(operation,input.toString()));
        if(!r.getBoolean("ok"))throw new Exception(r.getString("error"));
        return r.getJSONObject("data");
    }
}
